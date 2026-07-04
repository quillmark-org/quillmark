#[path = "common/mod.rs"]
mod common;
use common::demo;

fn main() {
    demo("usaf_memo", "usaf_memo_output.pdf").expect("Demo failed");
}
