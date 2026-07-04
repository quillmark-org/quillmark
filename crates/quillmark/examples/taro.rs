#[path = "common/mod.rs"]
mod common;
use common::demo;

fn main() {
    demo("taro", "taro.pdf").expect("Demo failed");
}
