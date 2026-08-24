#![allow(dead_code, unused_imports)]

//! This is a test file, where I test the user-friendliness of my library
mod examples;
use crate::examples::gltf_test::main as gltf_main;
use crate::examples::shaders::main as shaders_main;
use crate::examples::stress_pbr::main as pbr_main;

fn main() {
    // Comment out the function you don't want to run, and uncomment the one you do!

    shaders_main();
    // physics_main();

    // 10,000 cubes stress testing
    // unlit_main();
    // pbr_main();

    // rotate test
    // rotate_main();

    // gltf_main();
}
