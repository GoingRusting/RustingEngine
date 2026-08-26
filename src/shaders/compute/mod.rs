pub mod cs_test {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/shaders/compute/basic.comp",
    }
}

pub mod cs_space {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/shaders/compute/space.comp",
    }
}

pub mod cs_grid_build {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/shaders/compute/grid_build.comp",
    }
}

pub mod cs_empty {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/shaders/compute/empty.comp",
    }
}

pub mod cs_full {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/shaders/compute/full.comp"
    }
}

pub mod cs_no_rot {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/shaders/compute/mid.comp"
    }
}

pub mod cs_no_coll {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/shaders/compute/no_coll.comp"
    }
}

pub mod cs_cull {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/shaders/compute/cull.comp"
    }
}
