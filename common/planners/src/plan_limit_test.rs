// Copyright 2020-2021 The Datafuse Authors.
//
// SPDX-License-Identifier: Apache-2.0.

use std::sync::Arc;

use crate::*;

#[test]
fn test_limit_plan() -> anyhow::Result<()> {
    use pretty_assertions::assert_eq;

    let limit = PlanNode::Limit(LimitPlan {
        n: Some(33),
        offset: 0,
        input: Arc::from(PlanBuilder::empty().build()?),
    });
    let expect = "Limit: 33";
    let actual = format!("{:?}", limit);
    assert_eq!(expect, actual);
    Ok(())
}
