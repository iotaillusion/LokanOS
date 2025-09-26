use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;

use axum::http::Method;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Admin,
    Member,
    Guest,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Owner => "Owner",
            Role::Admin => "Admin",
            Role::Member => "Member",
            Role::Guest => "Guest",
        }
    }
}

impl FromStr for Role {
    type Err = PolicyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "owner" => Ok(Role::Owner),
            "admin" => Ok(Role::Admin),
            "member" => Ok(Role::Member),
            "guest" => Ok(Role::Guest),
            _ => Err(PolicyError::UnknownRole(s.to_string())),
        }
    }
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("failed to load RBAC policy: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse RBAC policy: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("unknown role {0}")]
    UnknownRole(String),
    #[error("invalid HTTP method {0}")]
    InvalidMethod(String),
}

#[derive(Debug, Clone)]
pub struct RbacPolicy {
    rules: Vec<RouteRule>,
    inherit: HashMap<Role, HashSet<Role>>, // maps to roles it inherits from
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteSummary {
    pub pattern: String,
    pub methods: Vec<String>,
    pub allowed_roles: Vec<String>,
    pub audit_action: Option<String>,
}

#[derive(Debug, Clone)]
struct RouteRule {
    pattern: RoutePattern,
    methods: HashSet<Method>,
    roles: HashSet<Role>,
    audit_action: Option<String>,
}

#[derive(Debug, Clone)]
struct RoutePattern(String);

#[derive(Debug, Clone)]
pub struct Decision {
    pub allowed: bool,
    pub audit_action: Option<String>,
}

impl RbacPolicy {
    pub fn from_path(path: &Path) -> Result<Self, PolicyError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Self::from_reader(reader)
    }

    fn from_reader<R: std::io::Read>(reader: R) -> Result<Self, PolicyError> {
        let raw: RawPolicy = serde_yaml::from_reader(reader)?;
        Self::try_from(raw)
    }

    pub fn authorize(&self, role: Role, method: &Method, path: &str) -> Decision {
        let effective_roles = self.expand_roles(role);
        for rule in &self.rules {
            if rule.matches(method, path) {
                let allowed = rule.roles.iter().any(|r| effective_roles.contains(r));
                return Decision {
                    allowed,
                    audit_action: rule.audit_action.clone(),
                };
            }
        }

        Decision {
            allowed: false,
            audit_action: None,
        }
    }

    fn expand_roles(&self, role: Role) -> HashSet<Role> {
        let mut visited = HashSet::new();
        let mut stack = vec![role];
        while let Some(current) = stack.pop() {
            if visited.insert(current) {
                if let Some(parents) = self.inherit.get(&current) {
                    for parent in parents {
                        stack.push(*parent);
                    }
                }
            }
        }
        visited
    }

    pub fn summaries(&self) -> Vec<RouteSummary> {
        let mut summaries: Vec<RouteSummary> = self
            .rules
            .iter()
            .map(|rule| {
                let mut methods: Vec<String> = rule
                    .methods
                    .iter()
                    .map(|method| method.as_str().to_string())
                    .collect();
                methods.sort();

                let mut roles: Vec<String> = rule
                    .roles
                    .iter()
                    .map(|role| role.as_str().to_string())
                    .collect();
                roles.sort();

                RouteSummary {
                    pattern: rule.pattern.0.clone(),
                    methods,
                    allowed_roles: roles,
                    audit_action: rule.audit_action.clone(),
                }
            })
            .collect();
        summaries.sort_by(|a, b| a.pattern.cmp(&b.pattern));
        summaries
    }
}

impl RouteRule {
    fn matches(&self, method: &Method, path: &str) -> bool {
        self.methods.contains(method) && self.pattern.matches(path)
    }
}

impl RoutePattern {
    fn matches(&self, path: &str) -> bool {
        if self.0.ends_with('*') {
            let prefix = &self.0[..self.0.len() - 1];
            path.starts_with(prefix)
        } else {
            self.0 == path
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawPolicy {
    roles: HashMap<String, RawRole>,
    routes: Vec<RawRoute>,
}

#[derive(Debug, Deserialize)]
struct RawRole {
    #[serde(default)]
    inherits: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawRoute {
    pattern: String,
    #[serde(default)]
    methods: Vec<String>,
    roles: Vec<String>,
    #[serde(default)]
    audit_action: Option<String>,
}

impl TryFrom<RawPolicy> for RbacPolicy {
    type Error = PolicyError;

    fn try_from(policy: RawPolicy) -> Result<Self, Self::Error> {
        let mut inherit: HashMap<Role, HashSet<Role>> = HashMap::new();
        for (role_name, definition) in policy.roles {
            let role = Role::from_str(&role_name)?;
            let parents: HashSet<Role> = definition
                .inherits
                .iter()
                .map(|name| Role::from_str(name))
                .collect::<Result<_, _>>()?;
            inherit.insert(role, parents);
        }

        let mut rules = Vec::new();
        for raw_rule in policy.routes {
            let methods: HashSet<Method> = if raw_rule.methods.is_empty() {
                let mut set = HashSet::new();
                set.insert(Method::GET);
                set
            } else {
                raw_rule
                    .methods
                    .iter()
                    .map(|method| {
                        Method::from_bytes(method.as_bytes())
                            .map_err(|_| PolicyError::InvalidMethod(method.clone()))
                    })
                    .collect::<Result<_, _>>()?
            };

            let roles = raw_rule
                .roles
                .iter()
                .map(|role| Role::from_str(role))
                .collect::<Result<_, _>>()?;

            let rule = RouteRule {
                pattern: RoutePattern(raw_rule.pattern.clone()),
                methods,
                roles,
                audit_action: raw_rule.audit_action.clone(),
            };
            rules.push(rule);
        }

        Ok(Self { rules, inherit })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_by_default() {
        let yaml = r#"
roles:
  owner:
    inherits: [admin]
  admin:
    inherits: [member]
  member:
    inherits: [guest]
  guest: {}
routes:
  - pattern: /v1/info
    methods: [GET]
    roles: [guest]
  - pattern: /v1/devices
    methods: [GET]
    roles: [member]
"#;
        let policy = RbacPolicy::from_reader(yaml.as_bytes()).expect("policy");
        let get = Method::GET;
        assert!(policy.authorize(Role::Guest, &get, "/v1/info").allowed);
        assert!(!policy.authorize(Role::Guest, &get, "/v1/devices").allowed);
        assert!(policy.authorize(Role::Member, &get, "/v1/devices").allowed);
    }
}
