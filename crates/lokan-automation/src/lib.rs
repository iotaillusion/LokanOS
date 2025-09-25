use std::{collections::HashMap, sync::Arc};

use lokan_event::{Event, EventBus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchOperator {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCondition {
    pub json_pointer: String,
    pub operator: MatchOperator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionKind {
    EmitEvent {
        topic: String,
        payload_template: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAction {
    pub kind: ActionKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub description: String,
    pub trigger_topic: String,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    pub enabled: bool,
}

impl Rule {
    pub fn matches(&self, event: &Event) -> bool {
        if !self.enabled || self.trigger_topic != event.topic {
            return false;
        }

        self.conditions.iter().all(|condition| {
            match event.payload.pointer(&condition.json_pointer) {
                Some(value) => evaluate_condition(value, &condition.operator, &condition.value),
                None => false,
            }
        })
    }
}

fn evaluate_condition(
    actual: &serde_json::Value,
    operator: &MatchOperator,
    expected: &serde_json::Value,
) -> bool {
    match operator {
        MatchOperator::Equals => actual == expected,
        MatchOperator::NotEquals => actual != expected,
        MatchOperator::GreaterThan => compare_f64(actual, expected, |a, b| a > b),
        MatchOperator::LessThan => compare_f64(actual, expected, |a, b| a < b),
        MatchOperator::Contains => match (actual, expected) {
            (serde_json::Value::Array(values), serde_json::Value::String(needle)) => {
                values.iter().any(|v| v == needle)
            }
            (serde_json::Value::String(haystack), serde_json::Value::String(needle)) => {
                haystack.contains(needle)
            }
            _ => false,
        },
    }
}

fn compare_f64<F>(lhs: &serde_json::Value, rhs: &serde_json::Value, predicate: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    match (lhs.as_f64(), rhs.as_f64()) {
        (Some(a), Some(b)) => predicate(a, b),
        _ => false,
    }
}

#[derive(Debug, Error)]
pub enum RuleError {
    #[error("rule with id {0} already exists")]
    AlreadyExists(String),
    #[error("rule with id {0} not found")]
    NotFound(String),
    #[error("event bus disconnected")]
    BusClosed,
}

#[derive(Clone)]
pub struct RuleEngine {
    rules: Arc<RwLock<HashMap<String, Rule>>>,
    event_bus: EventBus,
}

impl RuleEngine {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            rules: Arc::new(RwLock::new(HashMap::new())),
            event_bus,
        }
    }

    pub async fn register_rule(&self, rule: Rule) -> Result<(), RuleError> {
        let mut rules = self.rules.write().await;
        if rules.contains_key(&rule.id) {
            return Err(RuleError::AlreadyExists(rule.id));
        }
        info!(rule_id = %rule.id, "rule registered");
        rules.insert(rule.id.clone(), rule);
        Ok(())
    }

    pub async fn remove_rule(&self, rule_id: &str) -> Result<(), RuleError> {
        let mut rules = self.rules.write().await;
        if rules.remove(rule_id).is_none() {
            return Err(RuleError::NotFound(rule_id.into()));
        }
        info!(rule_id, "rule removed");
        Ok(())
    }

    pub async fn process_event(&self, event: &Event) {
        let rules = self.rules.read().await;
        for rule in rules.values() {
            if rule.matches(event) {
                debug!(rule_id = %rule.id, topic = %event.topic, "rule matched event");
                self.execute_actions(rule, event).await;
            }
        }
    }

    async fn execute_actions(&self, rule: &Rule, event: &Event) {
        for action in &rule.actions {
            match &action.kind {
                ActionKind::EmitEvent {
                    topic,
                    payload_template,
                } => {
                    let payload = render_template(payload_template.clone(), event);
                    self.event_bus.publish(Event::new(topic.clone(), payload));
                }
            }
        }
    }

    pub async fn run(self: Arc<Self>) -> Result<(), RuleError> {
        let mut rx = self.event_bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => self.process_event(&event).await,
                Err(_) => return Err(RuleError::BusClosed),
            }
        }
    }

    pub async fn list_rules(&self) -> Vec<Rule> {
        let rules = self.rules.read().await;
        rules.values().cloned().collect()
    }
}

fn render_template(mut template: serde_json::Value, event: &Event) -> serde_json::Value {
    match &mut template {
        serde_json::Value::String(value) => {
            let rendered = value.replace("{{event.topic}}", &event.topic);
            let payload_str = event.payload.to_string();
            let rendered = rendered.replace("{{event.payload}}", &payload_str);
            serde_json::Value::String(rendered)
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                *value = render_template(value.clone(), event);
            }
            serde_json::Value::Object(map.clone())
        }
        serde_json::Value::Array(items) => {
            let rendered_items = items
                .iter()
                .cloned()
                .map(|item| render_template(item, event))
                .collect();
            serde_json::Value::Array(rendered_items)
        }
        _ => template,
    }
}

/// Helper to construct a simple rule that echoes events.
pub fn create_echo_rule(topic: &str) -> Rule {
    Rule {
        id: format!("echo:{}", topic),
        description: "Echo any event back to the same topic".into(),
        trigger_topic: topic.to_string(),
        conditions: vec![],
        actions: vec![RuleAction {
            kind: ActionKind::EmitEvent {
                topic: topic.to_string(),
                payload_template: json!({
                    "echo": "{{event.payload}}",
                }),
            },
        }],
        enabled: true,
    }
}
