// Copyright 2020-2021 The Datafuse Authors.
//
// SPDX-License-Identifier: Apache-2.0.

use common_metatypes::Database;
use common_metatypes::SeqValue;
use common_runtime::tokio;
use pretty_assertions::assert_eq;

use crate::meta_service::state_machine::Replication;
use crate::meta_service::AppliedState;
use crate::meta_service::Cmd;
use crate::meta_service::LogEntry;
use crate::meta_service::Node;
use crate::meta_service::Slot;
use crate::meta_service::StateMachine;

#[test]
fn test_state_machine_assign_rand_nodes_to_slot() -> anyhow::Result<()> {
    // - Create a meta with 3 node 1,3,5.
    // - Assert that expected number of nodes are assigned to a slot.

    let mut meta = StateMachine {
        slots: vec![Slot::default(), Slot::default(), Slot::default()],
        nodes: maplit::hashmap! {
            1=> Node{..Default::default()},
            3=> Node{..Default::default()},
            5=> Node{..Default::default()},
        },
        replication: Replication::Mirror(3),
        ..Default::default()
    };

    // assign all node to slot 2
    meta.assign_rand_nodes_to_slot(2)?;
    assert_eq!(meta.slots[2].node_ids, vec![1, 3, 5]);

    // assign all node again to slot 2
    meta.assign_rand_nodes_to_slot(2)?;
    assert_eq!(meta.slots[2].node_ids, vec![1, 3, 5]);

    // assign 1 node again to slot 1
    meta.replication = Replication::Mirror(1);
    meta.assign_rand_nodes_to_slot(1)?;
    assert_eq!(1, meta.slots[1].node_ids.len());

    let id = meta.slots[1].node_ids[0];
    assert!(id == 1 || id == 3 || id == 5);

    Ok(())
}

#[test]
fn test_state_machine_init_slots() -> anyhow::Result<()> {
    // - Create a meta with 3 node 1,3,5.
    // - Initialize all slots.
    // - Assert slot states.

    let mut meta = StateMachine {
        slots: vec![Slot::default(), Slot::default(), Slot::default()],
        nodes: maplit::hashmap! {
            1=> Node{..Default::default()},
            3=> Node{..Default::default()},
            5=> Node{..Default::default()},
        },
        replication: Replication::Mirror(1),
        ..Default::default()
    };

    meta.init_slots()?;
    for slot in meta.slots.iter() {
        assert_eq!(1, slot.node_ids.len());

        let id = slot.node_ids[0];
        assert!(id == 1 || id == 3 || id == 5);
    }

    Ok(())
}

#[test]
fn test_state_machine_builder() -> anyhow::Result<()> {
    // - Assert default meta builder
    // - Assert customized meta builder

    let m = StateMachine::builder().build()?;
    assert_eq!(3, m.slots.len());
    let n = match m.replication {
        Replication::Mirror(x) => x,
    };
    assert_eq!(1, n);

    let m = StateMachine::builder()
        .slots(5)
        .mirror_replication(8)
        .build()?;
    assert_eq!(5, m.slots.len());
    let n = match m.replication {
        Replication::Mirror(x) => x,
    };
    assert_eq!(8, n);
    Ok(())
}

#[test]
fn test_state_machine_apply_non_dup_incr_seq() -> anyhow::Result<()> {
    let mut m = StateMachine::builder().build()?;

    for i in 0..3 {
        // incr "foo"

        let resp = m.apply_non_dup(&LogEntry {
            txid: None,
            cmd: Cmd::IncrSeq {
                key: "foo".to_string(),
            },
        })?;
        assert_eq!(AppliedState::Seq { seq: i + 1 }, resp);

        // incr "bar"

        let resp = m.apply_non_dup(&LogEntry {
            txid: None,
            cmd: Cmd::IncrSeq {
                key: "bar".to_string(),
            },
        })?;
        assert_eq!(AppliedState::Seq { seq: i + 1 }, resp);
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_state_machine_apply_incr_seq() -> anyhow::Result<()> {
    common_tracing::init_default_tracing();

    let mut sm = StateMachine::default();

    let cases = crate::meta_service::raftmeta_test::cases_incr_seq();

    for (name, txid, k, want) in cases.iter() {
        let resp = sm.apply(5, &LogEntry {
            txid: txid.clone(),
            cmd: Cmd::IncrSeq { key: k.to_string() },
        });
        assert_eq!(AppliedState::Seq { seq: *want }, resp.unwrap(), "{}", name);
    }

    Ok(())
}

#[test]
fn test_state_machine_apply_add_database() -> anyhow::Result<()> {
    let mut m = StateMachine::builder().build()?;

    struct T {
        name: &'static str,
        prev: Option<Database>,
        result: Option<Database>,
    }

    fn case(name: &'static str, prev: Option<u64>, result: Option<u64>) -> T {
        let prev = match prev {
            None => None,
            Some(id) => Some(Database {
                database_id: id,
                ..Default::default()
            }),
        };
        let result = match result {
            None => None,
            Some(id) => Some(Database {
                database_id: id,
                ..Default::default()
            }),
        };
        T { name, prev, result }
    }

    let cases: Vec<T> = vec![
        case("foo", None, Some(1)),
        case("foo", Some(1), None),
        case("bar", None, Some(2)),
        case("bar", Some(2), None),
        case("wow", None, Some(3)),
    ];

    for c in cases.iter() {
        // add

        let resp = m.apply_non_dup(&LogEntry {
            txid: None,
            cmd: Cmd::AddDatabase {
                name: c.name.to_string(),
            },
        })?;
        assert_eq!(
            AppliedState::DataBase {
                prev: c.prev.clone(),
                result: c.result.clone(),
            },
            resp
        );

        // get

        let want = match (&c.prev, &c.result) {
            (Some(ref a), _) => a.database_id,
            (_, Some(ref b)) => b.database_id,
            _ => {
                panic!("both none");
            }
        };

        let got = m
            .get_database(c.name)
            .ok_or_else(|| anyhow::anyhow!("db not found: {}", c.name));
        assert_eq!(want, got.unwrap().database_id);
    }

    Ok(())
}

#[test]
fn test_state_machine_apply_non_dup_generic_kv() -> anyhow::Result<()> {
    let mut m = StateMachine::builder().build()?;

    struct T {
        // input:
        key: String,
        seq: Option<u64>,
        value: Vec<u8>,
        // want:
        prev: Option<SeqValue>,
        result: Option<SeqValue>,
    }

    fn case(
        name: &'static str,
        seq: Option<u64>,
        value: &'static str,
        prev: Option<(u64, &'static str)>,
        result: Option<(u64, &'static str)>,
    ) -> T {
        let name = name.to_string();
        let value = value.to_string().into_bytes();
        let prev = match prev {
            None => None,
            Some((s, v)) => Some((s, v.to_string().into_bytes())),
        };
        let result = match result {
            None => None,
            Some((s, v)) => Some((s, v.to_string().into_bytes())),
        };
        T {
            key: name,
            seq,
            value,
            prev,
            result,
        }
    }

    let cases: Vec<T> = vec![
        case("foo", Some(5), "b", None, None),
        case("foo", None, "a", None, Some((1, "a"))),
        case("foo", None, "b", Some((1, "a")), Some((2, "b"))),
        case("foo", Some(5), "b", Some((2, "b")), None),
        case("bar", Some(0), "x", None, Some((3, "x"))),
        case("bar", Some(0), "y", Some((3, "x")), None),
    ];

    for (i, c) in cases.iter().enumerate() {
        let mes = format!("{}-th: {}({:?})={:?}", i, c.key, c.seq, c.value);

        // write

        let resp = m.apply_non_dup(&LogEntry {
            txid: None,
            cmd: Cmd::UpsertKV {
                key: c.key.clone(),
                seq: c.seq,
                value: c.value.clone(),
            },
        })?;
        assert_eq!(
            AppliedState::KV {
                prev: c.prev.clone(),
                result: c.result.clone(),
            },
            resp,
            "write: {}",
            mes,
        );

        // get

        let want = match (&c.prev, &c.result) {
            (_, Some(ref b)) => Some(b.clone()),
            (Some(ref a), _) => Some(a.clone()),
            _ => None,
        };

        let got = m.get_kv(&c.key);
        assert_eq!(want, got, "get: {}", mes,);
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_state_machine_apply_add_file() -> anyhow::Result<()> {
    common_tracing::init_default_tracing();

    let mut sm = StateMachine::default();

    let cases = crate::meta_service::raftmeta_test::cases_add_file();

    for (name, txid, k, v, want_prev, want_result) in cases.iter() {
        let resp = sm.apply(5, &LogEntry {
            txid: txid.clone(),
            cmd: Cmd::AddFile {
                key: k.to_string(),
                value: v.to_string(),
            },
        });
        assert_eq!(
            AppliedState::String {
                prev: want_prev.clone(),
                result: want_result.clone()
            },
            resp.unwrap(),
            "{}",
            name
        );
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_state_machine_apply_set_file() -> anyhow::Result<()> {
    common_tracing::init_default_tracing();

    let mut sm = StateMachine::default();

    let cases = crate::meta_service::raftmeta_test::cases_set_file();

    for (name, txid, k, v, want_prev, want_result) in cases.iter() {
        let resp = sm.apply(5, &LogEntry {
            txid: txid.clone(),
            cmd: Cmd::SetFile {
                key: k.to_string(),
                value: v.to_string(),
            },
        });
        assert_eq!(
            AppliedState::String {
                prev: want_prev.clone(),
                result: want_result.clone()
            },
            resp.unwrap(),
            "{}",
            name
        );
    }

    Ok(())
}
