pub mod camera;
pub mod compute_registry;
pub mod frame_pacer;
pub mod pipeline;
pub mod render;
pub mod scene_renderer;
pub mod shader_registry;
pub mod swapchain;

use std::sync::Arc;
use vulkano::device::physical::PhysicalDeviceType;
use vulkano::device::{
    Device, DeviceCreateInfo, DeviceExtensions, Queue, QueueCreateInfo,
    QueueFlags,
};
use vulkano::instance::debug::ValidationFeatureEnable;
use vulkano::instance::debug::{
    DebugUtilsMessageSeverity, DebugUtilsMessageType, DebugUtilsMessenger,
    DebugUtilsMessengerCallback, DebugUtilsMessengerCreateInfo,
};
use vulkano::instance::{Instance, InstanceCreateInfo};
use vulkano::swapchain::Surface;
use vulkano::VulkanLibrary;
use winit::event_loop::EventLoop;
use winit::window::Window;

#[derive(Clone)]
pub struct VulkanBase {
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
    pub surface: Arc<Surface>,
    pub window: Arc<Window>,
    pub instance: Arc<Instance>,
    pub debug_messenger: Option<Arc<DebugUtilsMessenger>>,
}

pub fn init_vulkan(event_loop: &EventLoop<()>, title: &str) -> VulkanBase {
    let library = VulkanLibrary::new().expect("No Vulkan driver found.");
    let mut required_extensions = Surface::required_extensions(event_loop)
        .expect("Failed to determine Vulkan surface extensions");

    let validation_enabled = cfg!(debug_assertions)
        && library
            .layer_properties()
            .expect("Failed to enumerate Vulkan layers")
            .any(|layer| layer.name() == "VK_LAYER_KHRONOS_validation");
    let validation_callback = unsafe {
        DebugUtilsMessengerCallback::new(|severity, message_type, data| {
            eprintln!(
                "[Vulkan {severity:?} {message_type:?}] {}: {}",
                data.message_id_name.unwrap_or("unknown"),
                data.message
            );
        })
    };
    let debug_create_info = DebugUtilsMessengerCreateInfo {
        message_severity: DebugUtilsMessageSeverity::ERROR
            | DebugUtilsMessageSeverity::WARNING
            | DebugUtilsMessageSeverity::INFO,
        message_type: DebugUtilsMessageType::GENERAL
            | DebugUtilsMessageType::VALIDATION
            | DebugUtilsMessageType::PERFORMANCE,
        ..DebugUtilsMessengerCreateInfo::user_callback(validation_callback)
    };

    let mut enabled_validation_features = Vec::new();
    if validation_enabled
        && library.supported_extensions().ext_validation_features
    {
        required_extensions.ext_validation_features = true;
        enabled_validation_features
            .push(ValidationFeatureEnable::SynchronizationValidation);
        let gpu_validation_enabled = std::env::var("RUSTING_GPU_VALIDATION")
            .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE"));
        if gpu_validation_enabled {
            enabled_validation_features
                .push(ValidationFeatureEnable::GpuAssisted);
            enabled_validation_features
                .push(ValidationFeatureEnable::GpuAssistedReserveBindingSlot);
        }
    }
    let debug_utils_enabled =
        validation_enabled && library.supported_extensions().ext_debug_utils;
    if debug_utils_enabled {
        required_extensions.ext_debug_utils = true;
    }

    let instance = Instance::new(
        library,
        InstanceCreateInfo {
            enabled_extensions: required_extensions,
            enabled_layers: validation_enabled
                .then(|| "VK_LAYER_KHRONOS_validation".to_owned())
                .into_iter()
                .collect(),
            debug_utils_messengers: debug_utils_enabled
                .then(|| debug_create_info.clone())
                .into_iter()
                .collect(),
            enabled_validation_features,
            ..Default::default()
        },
    )
    .unwrap();

    #[allow(deprecated)]
    let window = Arc::new(
        event_loop
            .create_window(Window::default_attributes().with_title(title))
            .expect("Failed to create window"),
    );
    let surface = Surface::from_window(instance.clone(), window.clone())
        .expect("Failed to create Vulkan surface");
    let debug_messenger = debug_utils_enabled
        .then(|| {
            DebugUtilsMessenger::new(instance.clone(), debug_create_info).ok()
        })
        .flatten()
        .map(Arc::new);

    let device_extensions = DeviceExtensions {
        khr_swapchain: true,
        ..DeviceExtensions::empty()
    };

    let (physical_device, queue_family_index) = instance
        .enumerate_physical_devices()
        .unwrap()
        .filter(|p| p.supported_extensions().contains(&device_extensions))
        .filter_map(|p| {
            p.queue_family_properties()
                .iter()
                .enumerate()
                .position(|(i, q)| {
                    q.queue_flags.intersects(QueueFlags::GRAPHICS)
                        && p.surface_support(i as u32, &surface)
                            .unwrap_or(false)
                })
                .map(|i| (p, i as u32))
        })
        .min_by_key(|(p, _)| match p.properties().device_type {
            PhysicalDeviceType::DiscreteGpu => 0,
            PhysicalDeviceType::IntegratedGpu => 1,
            _ => 2,
        })
        .unwrap();

    let (device, mut queues) = Device::new(
        physical_device,
        DeviceCreateInfo {
            enabled_extensions: device_extensions,
            queue_create_infos: vec![QueueCreateInfo {
                queue_family_index,
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .unwrap();

    let queue = queues.next().unwrap();

    VulkanBase {
        device,
        queue,
        surface,
        window,
        instance,
        debug_messenger,
    }
}
