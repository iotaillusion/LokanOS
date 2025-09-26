use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;

use updater::{HealthCheckError, HealthClient, MemoryStateStore, Slot, SlotState, UpdaterCore};

#[derive(Debug, Clone)]
struct TestStep {
    action: Action,
}

#[derive(Debug, Clone)]
enum Action {
    Stage { artifact: &'static str },
    Commit { expect_ok: bool },
    MarkBad { expect_some: bool },
    Rollback { expect_ok: bool },
}

#[derive(Debug)]
struct TestCase {
    name: &'static str,
    steps: Vec<TestStep>,
    health_results: Vec<bool>,
    expected: Expected,
}

#[derive(Debug)]
struct Expected {
    active: Option<Slot>,
    staging: Option<Slot>,
    last_failed: Option<Slot>,
    slots: Vec<(Slot, SlotState)>,
}

#[tokio::test]
async fn state_machine_transitions() {
    let cases = vec![
        TestCase {
            name: "happy_path_commit",
            steps: vec![stage_step("artifact:v1"), commit_step(true)],
            health_results: vec![true],
            expected: Expected {
                active: Some(Slot::B),
                staging: None,
                last_failed: None,
                slots: vec![(Slot::A, SlotState::Inactive), (Slot::B, SlotState::Active)],
            },
        },
        TestCase {
            name: "commit_failure_marks_bad",
            steps: vec![stage_step("artifact:v2"), commit_step(false)],
            health_results: vec![false],
            expected: Expected {
                active: Some(Slot::A),
                staging: None,
                last_failed: Some(Slot::B),
                slots: vec![(Slot::A, SlotState::Active), (Slot::B, SlotState::Bad)],
            },
        },
        TestCase {
            name: "mark_bad_then_rollback",
            steps: vec![
                stage_step("artifact:v3"),
                commit_step(true),
                TestStep {
                    action: Action::MarkBad { expect_some: true },
                },
                TestStep {
                    action: Action::Rollback { expect_ok: true },
                },
            ],
            health_results: vec![true],
            expected: Expected {
                active: Some(Slot::A),
                staging: None,
                last_failed: None,
                slots: vec![(Slot::A, SlotState::Active), (Slot::B, SlotState::Inactive)],
            },
        },
    ];

    for case in cases {
        let store = Arc::new(MemoryStateStore::default()) as Arc<dyn updater::StateStore>;
        let health_client = Arc::new(SequenceHealthClient::new(case.health_results.clone()));
        let core = UpdaterCore::new(store, health_client, Vec::new(), Duration::from_secs(1), 0)
            .await
            .expect("core init");

        run_steps(&case, &core).await;
        assert_state(&case, &core).await;
    }
}

fn stage_step(artifact: &'static str) -> TestStep {
    TestStep {
        action: Action::Stage { artifact },
    }
}

fn commit_step(expect_ok: bool) -> TestStep {
    TestStep {
        action: Action::Commit { expect_ok },
    }
}

async fn run_steps(case: &TestCase, core: &UpdaterCore) {
    for step in &case.steps {
        match step.action {
            Action::Stage { artifact } => {
                core.stage(artifact.to_string())
                    .await
                    .unwrap_or_else(|err| {
                        panic!("{name}: stage failed: {err}", name = case.name, err = err)
                    });
            }
            Action::Commit { expect_ok } => {
                let result = core.commit_on_health().await;
                assert_eq!(
                    result.is_ok(),
                    expect_ok,
                    "{}: commit expectation",
                    case.name
                );
            }
            Action::MarkBad { expect_some } => {
                let result = core.mark_bad().await.unwrap_or_else(|err| {
                    panic!(
                        "{name}: mark_bad failed: {err}",
                        name = case.name,
                        err = err
                    )
                });
                assert_eq!(
                    result.is_some(),
                    expect_some,
                    "{}: mark_bad expectation",
                    case.name
                );
            }
            Action::Rollback { expect_ok } => {
                let result = core.rollback().await;
                assert_eq!(
                    result.is_ok(),
                    expect_ok,
                    "{}: rollback expectation",
                    case.name
                );
            }
        }
    }
}

async fn assert_state(case: &TestCase, core: &UpdaterCore) {
    let state = core.state().await;
    assert_eq!(
        state.active, case.expected.active,
        "{}: active slot",
        case.name
    );
    assert_eq!(
        state.staging, case.expected.staging,
        "{}: staging slot",
        case.name
    );
    assert_eq!(
        state.last_failed, case.expected.last_failed,
        "{}: last failed",
        case.name
    );

    for (slot, expected_state) in &case.expected.slots {
        let info = state
            .slots
            .get(slot)
            .unwrap_or_else(|| panic!("{}: missing slot {slot:?}", case.name));
        assert_eq!(
            info.state, *expected_state,
            "{}: slot {slot:?} state",
            case.name
        );
    }
}

#[derive(Debug)]
struct SequenceHealthClient {
    results: Mutex<VecDeque<bool>>,
}

impl SequenceHealthClient {
    fn new(results: Vec<bool>) -> Self {
        Self {
            results: Mutex::new(results.into_iter().collect()),
        }
    }
}

#[async_trait]
impl HealthClient for SequenceHealthClient {
    async fn wait_for_quorum(
        &self,
        _endpoints: &[String],
        _deadline: Duration,
        _quorum: usize,
    ) -> Result<bool, HealthCheckError> {
        let mut guard = self.results.lock().await;
        Ok(guard.pop_front().unwrap_or(true))
    }
}
