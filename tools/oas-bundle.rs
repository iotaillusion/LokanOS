use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use serde_yaml::{Mapping, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .ok_or_else(|| anyhow!("tools directory is expected to live under the repo root"))?;
    let openapi_dir = repo_root.join("openapi");

    if !openapi_dir.exists() {
        return Err(anyhow!(
            "openapi directory not found at {}",
            openapi_dir.display()
        ));
    }

    let mut service_docs = Vec::new();

    for entry in fs::read_dir(&openapi_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('_') {
                continue;
            }
            if !(name.ends_with(".yaml") || name.ends_with(".yml")) {
                continue;
            }
        } else {
            continue;
        }

        let file = fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;
        let doc: Value = serde_yaml::from_reader(file)
            .with_context(|| format!("parsing YAML from {}", path.display()))?;
        service_docs.push((path, doc));
    }

    if service_docs.is_empty() {
        return Err(anyhow!("no service OpenAPI documents were discovered"));
    }

    service_docs.sort_by(|(a, _), (b, _)| a.file_name().cmp(&b.file_name()));

    let mut bundle_paths: IndexMap<String, IndexMap<String, Value>> = IndexMap::new();
    let mut bundle_components: IndexMap<String, IndexMap<String, Value>> = IndexMap::new();
    let mut bundle_tags: IndexMap<String, Value> = IndexMap::new();
    let mut operation_ids = HashSet::new();

    for (path, doc) in &service_docs {
        let doc_map = doc
            .as_mapping()
            .ok_or_else(|| anyhow!("{} must be a YAML mapping at the root", path.display()))?;

        validate_version(path, doc_map)?;
        collect_tags(path, doc_map, &mut bundle_tags)?;
        collect_components(path, doc_map, &mut bundle_components)?;
        collect_paths(path, doc_map, &mut bundle_paths, &mut operation_ids)?;
    }

    let mut bundle = Mapping::new();
    bundle.insert(Value::String("openapi".into()), Value::String("3.1.0".into()));

    let mut info = Mapping::new();
    info.insert(
        Value::String("title".into()),
        Value::String("LokanOS API Bundle".into()),
    );
    info.insert(Value::String("version".into()), Value::String("0.1.0".into()));
    bundle.insert(Value::String("info".into()), Value::Mapping(info));

    if !bundle_tags.is_empty() {
        let tags_seq: Vec<Value> = bundle_tags.into_iter().map(|(_, v)| v).collect();
        bundle.insert(Value::String("tags".into()), Value::Sequence(tags_seq));
    }

    let mut paths_map = Mapping::new();
    for (path_key, methods) in bundle_paths {
        let mut method_map = Mapping::new();
        for (method_key, operation) in methods {
            method_map.insert(Value::String(method_key), operation);
        }
        paths_map.insert(Value::String(path_key), Value::Mapping(method_map));
    }
    bundle.insert(Value::String("paths".into()), Value::Mapping(paths_map));

    if !bundle_components.is_empty() {
        let mut components_map = Mapping::new();
        for (comp_type, entries) in bundle_components {
            let mut entry_map = Mapping::new();
            for (name, value) in entries {
                entry_map.insert(Value::String(name), value);
            }
            components_map.insert(Value::String(comp_type), Value::Mapping(entry_map));
        }
        bundle.insert(Value::String("components".into()), Value::Mapping(components_map));
    }

    let bundle_path = openapi_dir.join("_bundle.yaml");
    let writer = fs::File::create(&bundle_path)
        .with_context(|| format!("creating {}", bundle_path.display()))?;
    serde_yaml::to_writer(writer, &Value::Mapping(bundle))
        .with_context(|| format!("writing bundle to {}", bundle_path.display()))?;

    // Basic syntax validation by parsing the freshly generated bundle.
    let reader = fs::File::open(&bundle_path)
        .with_context(|| format!("re-opening {} for validation", bundle_path.display()))?;
    let _: Value = serde_yaml::from_reader(reader)
        .with_context(|| format!("validating YAML syntax for {}", bundle_path.display()))?;

    println!(
        "Bundled {} documents into {}",
        service_docs.len(),
        bundle_path.display()
    );

    Ok(())
}

fn validate_version(path: &Path, doc: &Mapping) -> Result<()> {
    match doc.get(&Value::String("openapi".into())) {
        Some(Value::String(version)) if version.starts_with("3.1") => Ok(()),
        Some(Value::String(version)) => Err(anyhow!(
            "{} declares unsupported OpenAPI version {}",
            path.display(),
            version
        )),
        _ => Err(anyhow!(
            "{} is missing the required 'openapi' version field",
            path.display()
        )),
    }
}

fn collect_tags(
    path: &Path,
    doc: &Mapping,
    combined: &mut IndexMap<String, Value>,
) -> Result<()> {
    let Some(Value::Sequence(tags)) = doc.get(&Value::String("tags".into())) else {
        return Ok(());
    };

    for tag in tags {
        let tag_map = tag
            .as_mapping()
            .ok_or_else(|| anyhow!("{} contains a non-object tag entry", path.display()))?;
        let Some(Value::String(name)) = tag_map.get(&Value::String("name".into())) else {
            return Err(anyhow!(
                "{} defines a tag without the required 'name' field",
                path.display()
            ));
        };

        combined.entry(name.clone()).or_insert_with(|| tag.clone());
    }

    Ok(())
}

fn collect_components(
    path: &Path,
    doc: &Mapping,
    combined: &mut IndexMap<String, IndexMap<String, Value>>,
) -> Result<()> {
    let Some(Value::Mapping(components)) = doc.get(&Value::String("components".into())) else {
        return Ok(());
    };

    for (comp_type_value, entries_value) in components {
        let comp_type = comp_type_value
            .as_str()
            .ok_or_else(|| anyhow!("{} has a non-string components key", path.display()))?
            .to_string();
        let entries_map = entries_value
            .as_mapping()
            .ok_or_else(|| anyhow!(
                "{} component '{}' must be a mapping",
                path.display(),
                comp_type
            ))?;

        let component_bucket = combined
            .entry(comp_type)
            .or_insert_with(IndexMap::new);

        for (name_value, item_value) in entries_map {
            let name = name_value
                .as_str()
                .ok_or_else(|| anyhow!(
                    "{} component name must be a string",
                    path.display()
                ))?
                .to_string();

            if let Some(existing) = component_bucket.get(&name) {
                if existing != item_value {
                    return Err(anyhow!(
                        "{} defines component '{}' which conflicts with an existing definition",
                        path.display(),
                        name
                    ));
                }
            } else {
                component_bucket.insert(name, item_value.clone());
            }
        }
    }

    Ok(())
}

fn collect_paths(
    path: &Path,
    doc: &Mapping,
    combined: &mut IndexMap<String, IndexMap<String, Value>>,
    operation_ids: &mut HashSet<String>,
) -> Result<()> {
    let Some(Value::Mapping(paths)) = doc.get(&Value::String("paths".into())) else {
        return Ok(());
    };

    for (path_key, path_item_value) in paths {
        let path_str = path_key
            .as_str()
            .ok_or_else(|| anyhow!("{} path keys must be strings", path.display()))?
            .to_string();

        let path_item = path_item_value
            .as_mapping()
            .ok_or_else(|| anyhow!(
                "{} path '{}' must map to an object",
                path.display(),
                path_str
            ))?;

        let operations_bucket = combined
            .entry(path_str.clone())
            .or_insert_with(IndexMap::new);

        for (method_key, operation_value) in path_item {
            let method = method_key
                .as_str()
                .ok_or_else(|| anyhow!(
                    "{} path '{}' uses a non-string HTTP method",
                    path.display(),
                    path_str
                ))?
                .to_lowercase();

            if operations_bucket.contains_key(&method) {
                return Err(anyhow!(
                    "duplicate definition for {} {} in {}",
                    method.to_uppercase(),
                    path_str,
                    path.display()
                ));
            }

            let operation = operation_value.as_mapping().ok_or_else(|| {
                anyhow!(
                    "{} path '{}' method '{}' must define an object",
                    path.display(),
                    path_str,
                    method
                )
            })?;

            let operation_id = operation
                .get(&Value::String("operationId".into()))
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow!(
                        "{} path '{}' method '{}' is missing an operationId",
                        path.display(),
                        path_str,
                        method
                    )
                })?
                .to_string();

            if !operation_ids.insert(operation_id.clone()) {
                return Err(anyhow!("duplicate operationId '{}' detected", operation_id));
            }

            operations_bucket.insert(method, operation_value.clone());
        }
    }

    Ok(())
}
