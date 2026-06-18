use super::prelude::*;

#[test]
fn manifest_parses_global_tick_contract() {
    let manifest = parse_launch_manifest(
        r#"
{
  "package": "global_tick_demo",
  "ir_version": "0.1",
  "determinism": {
    "mode": "global_tick",
    "tick_timeout_ms": 1000,
    "on_timeout": "fault_graph",
    "processes": ["controller", "plant"]
  },
  "profiles": ["test"],
  "targets": ["linux"],
  "graphs": [
    {
      "name": "default",
      "processes": [
        {
          "name": "controller",
          "backend": "inproc",
          "runtime_kind": "rust"
        },
        {
          "name": "plant",
          "backend": "inproc",
          "runtime_kind": "rust"
        }
      ]
    }
  ]
}
"#,
    )
    .expect("global_tick manifest should parse");

    assert_eq!(manifest.determinism.mode, "global_tick");
    assert_eq!(manifest.determinism.tick_timeout_ms, 1000);
    assert_eq!(manifest.determinism.on_timeout, "fault_graph");
    assert_eq!(manifest.determinism.processes, vec!["controller", "plant"]);
}

#[test]
fn global_tick_coordinator_advances_after_all_participants_done() {
    let mut coordinator =
        GlobalTickCoordinator::new(vec!["controller".to_string(), "plant".to_string()], 1000);

    let grants = coordinator.start_tick(10);
    assert_eq!(
        grants,
        vec![
            TickCoordinatorEvent::Grant {
                participant: "controller".to_string(),
                grant: TickGrant {
                    tick_id: 1,
                    logical_time_ms: 10
                }
            },
            TickCoordinatorEvent::Grant {
                participant: "plant".to_string(),
                grant: TickGrant {
                    tick_id: 1,
                    logical_time_ms: 10
                }
            }
        ]
    );
    assert!(
        coordinator
            .mark_done(TickDone {
                tick_id: 1,
                participant: "controller".to_string()
            })
            .is_empty()
    );
    assert_eq!(
        coordinator.mark_done(TickDone {
            tick_id: 1,
            participant: "plant".to_string()
        }),
        vec![TickCoordinatorEvent::Completed { tick_id: 1 }]
    );
}

#[test]
fn global_tick_coordinator_faults_on_timeout() {
    let mut coordinator =
        GlobalTickCoordinator::new(vec!["controller".to_string(), "plant".to_string()], 1000);

    coordinator.start_tick(10);
    assert_eq!(
        coordinator.timeout("barrier timeout"),
        Some(TickCoordinatorEvent::Fault {
            tick_id: 1,
            reason: "barrier timeout".to_string()
        })
    );
    assert!(coordinator.start_tick(20).is_empty());
}
