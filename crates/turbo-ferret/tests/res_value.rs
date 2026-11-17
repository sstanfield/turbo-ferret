//! Port of TB's src/tb/tests/test_tb_value.cpp (the node part; `Value` itself is tested in
//! src/value.rs).

use turbo_ferret::res::ResNode;

#[test]
fn node_create_on_get() {
    let mut node = ResNode::default();
    assert!(node.get("foo>bar>funky").is_none());
    // TB: GetNode(..., GET_MISS_POLICY_CREATE) != nullptr
    let created = node.get_or_create("foo>bar>funky");
    assert_eq!(&*created.name, "funky");
    assert!(node.get("foo>bar>funky").is_some());
}
