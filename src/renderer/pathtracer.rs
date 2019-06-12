pub mod ray_triangle_intersector;
pub mod camera_ray_generator;
pub mod types;
pub mod accumulator;
pub mod vertex_skinner;
pub mod aabb_calculator;

use crate::window::WindowState;
use crate::renderer::core::backend::BackendState;
use crate::renderer::core::device::DeviceState;
use crate::renderer::core::swapchain::SwapchainState;
use crate::renderer::core::pipeline::PipelineState;
use crate::renderer::core::command::CommandState;
use crate::renderer::core::buffer::BufferState;
use crate::renderer::scene::Scene;
use crate::renderer::core::descriptor::DescSetLayout;
use self::camera_ray_generator::CameraRayGenerator;
use self::ray_triangle_intersector::RayTriangleIntersector;
use self::accumulator::Accumulator;
use self::vertex_skinner::VertexSkinner;
use self::aabb_calculator::AabbCalculator;
use crate::renderer::Renderer;
use self::types::Ray;
use self::types::Intersection;
use crate::window::DIMS;
use crate::renderer::WORK_GROUP_SIZE;

use gfx_hal::{Backend, Device, Submission, Swapchain, command, pso, format, image, memory, buffer, pool};

use std::cell::RefCell;
use std::rc::Rc;
use std::iter;
use std::path::Path;
use gfx_hal::pso::Stage::Vertex;
use gfx_backend_vulkan::CommandQueue;

pub struct Pathtracer<B: Backend> {
    pub swapchain: SwapchainState<B>,
    pub device: Rc<RefCell<DeviceState<B>>>,
    pub backend: BackendState<B>,
    pub window: WindowState,
    pub command: CommandState<B>,
    pub camera_ray_generator: CameraRayGenerator<B>,
    pub ray_triangle_intersector: RayTriangleIntersector<B>,
    pub vertex_skinner: VertexSkinner<B>,
    pub aabb_calculator: AabbCalculator<B>,
    pub accumulator: Accumulator<B>,
    pub camera_buffer: BufferState<B>,
    pub ray_buffer: BufferState<B>,
    pub vertex_in_buffer: BufferState<B>,
    pub vertex_out_buffer: BufferState<B>,
    pub index_buffer: BufferState<B>,
    pub intersection_buffer: BufferState<B>,
    pub aabb_buffer: BufferState<B>,
}

impl<B: Backend> Pathtracer<B> {

    pub unsafe fn new(mut backend: BackendState<B>, window: WindowState, scene: &Scene) -> Self {

        println!("creating render state");

        let device = Rc::new(RefCell::new(DeviceState::new(
            backend.adapter.adapter.take().unwrap(),
            &backend.surface,
        )));

        let mut swapchain = SwapchainState::new(&mut backend, Rc::clone(&device));
        println!("created swap chain");

        let number_of_images = swapchain.number_of_images();
        println!("backbuffer size: {:?}", number_of_images);

        let mut command = CommandState::new(
            Rc::clone(&device),
            number_of_images
        );
        println!("created command buffer state");

        let camera_ray_generator = CameraRayGenerator::new(Rc::clone(&device));

        let ray_triangle_intersector = RayTriangleIntersector::new(Rc::clone(&device));

        let accumulator = Accumulator::new(Rc::clone(&device));

        let vertex_skinner = VertexSkinner::new(Rc::clone(&device));

        let aabb_calculator = AabbCalculator::new(Rc::clone(&device));

        println!("memory types: {:?}", &backend.adapter.memory_types);

        let camera_buffer = BufferState::new(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::CPU_VISIBLE,
            buffer::Usage::STORAGE,
            &scene.camera_data()
        );

        let ray_buffer = BufferState::empty(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::DEVICE_LOCAL,
            buffer::Usage::STORAGE,
            (DIMS.width * DIMS.height) as u64,
            Ray{
                origin: [0.0, 0.0, 0.0, 0.0],
                direction: [0.0, 0.0, 0.0, 0.0],
            }
        );

        let intersection_buffer = BufferState::empty(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::DEVICE_LOCAL,
             buffer::Usage::STORAGE,
            (DIMS.width * DIMS.height) as u64,
            Intersection{
                color: [0.0, 0.0, 0.0, 0.0],
            }
        );

        let index_buffer = BufferState::empty(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::DEVICE_LOCAL,
             buffer::Usage::TRANSFER_DST | buffer::Usage::INDEX,
            scene.mesh_data.no_of_indices() as u64,
            types::Index(0),
        );

        println!("POSITIONS LEN: {:?}", scene.mesh_data.vertices.len());

        let vertex_in_buffer = BufferState::empty(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::DEVICE_LOCAL,
             buffer::Usage::TRANSFER_DST | buffer::Usage::VERTEX,
            scene.mesh_data.no_of_vertices() as u64,
            types::Vertex([0.0, 0.0, 0.0, 0.0])

        );

        let vertex_out_buffer = BufferState::empty(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::DEVICE_LOCAL,
            buffer::Usage::VERTEX,
            scene.mesh_data.no_of_vertices() as u64,
            types::Vertex([0.0, 0.0, 0.0, 0.0])

        );

        let aabb_buffer = BufferState::empty(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::DEVICE_LOCAL,
            buffer::Usage::STORAGE,
            1,
            types::Aabb{min: [0.0, 0.0, 0.0, 0.0], max: [0.0, 0.0, 0.0, 0.0]}
        );

        let staging_index_buffer = BufferState::new(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::CPU_VISIBLE,
            buffer::Usage::TRANSFER_SRC,
            &scene.mesh_data.indices,
        );

        let staging_vertex_buffer = BufferState::new(
            Rc::clone(&device),
            &backend.adapter.memory_types,
            memory::Properties::CPU_VISIBLE,
            buffer::Usage::TRANSFER_SRC,
            &scene.mesh_data.vertices,
        );

        camera_ray_generator.write_desc_set(
            Rc::clone(&device),
            camera_buffer.get_buffer(),
            ray_buffer.get_buffer(),
        );

        vertex_skinner.write_desc_set(
            Rc::clone(&device),
            camera_buffer.get_buffer(),
            vertex_in_buffer.get_buffer(),
            vertex_out_buffer.get_buffer(),
        );

        ray_triangle_intersector.write_desc_set(
            Rc::clone(&device),
            ray_buffer.get_buffer(),
            vertex_out_buffer.get_buffer(),
            index_buffer.get_buffer(),
            intersection_buffer.get_buffer(),
            aabb_buffer.get_buffer(),
        );

        accumulator.write_desc_set(
            Rc::clone(&device),
            intersection_buffer.get_buffer(),
        );

        accumulator.write_frame_desc_sets(
            Rc::clone(&device),
            swapchain.get_image_views(),
        );

        aabb_calculator.write_desc_set(
            Rc::clone(&device),
            vertex_out_buffer.get_buffer(),
            aabb_buffer.get_buffer(),
        );

        // Upload data
        unsafe {

            let mut transfered_image_fence = device.borrow().device.create_fence(false).expect("Can't create fence");

            let mut staging_pool = device
                .borrow()
                .device
                .create_command_pool_typed(
                    &device.borrow().queues,
                    pool::CommandPoolCreateFlags::empty(),
                )
                .expect("Can't create staging command pool");

            let mut cmd_buffer = staging_pool.acquire_command_buffer::<command::OneShot>();

            cmd_buffer.begin();

            cmd_buffer.copy_buffer(
                &staging_index_buffer.get_buffer(),
                &index_buffer.get_buffer(),
                &[
                    command::BufferCopy {
                        src: 0,
                        dst: 0,
                        size: staging_index_buffer.size,
                    },
                ],
            );

            cmd_buffer.copy_buffer(
                &staging_vertex_buffer.get_buffer(),
                &vertex_in_buffer.get_buffer(),
                &[
                    command::BufferCopy {
                        src: 0,
                        dst: 0,
                        size: staging_vertex_buffer.size,
                    },
                ],
            );

            cmd_buffer.finish();

            device.borrow_mut().queues.queues[0]
                .submit_nosemaphores(&[cmd_buffer], Some(&mut transfered_image_fence));

            device
                .borrow()
                .device
                .destroy_command_pool(staging_pool.into_raw());

        }



        Pathtracer {
            swapchain: swapchain,
            device: device,
            backend: backend,
            window: window,
            command,
            camera_ray_generator,
            ray_triangle_intersector,
            accumulator,
            vertex_skinner,
            camera_buffer,
            ray_buffer,
            index_buffer,
            vertex_in_buffer,
            vertex_out_buffer,
            intersection_buffer,
            aabb_calculator,
            aabb_buffer,
        }
    }

    pub fn render(&mut self, scene: &Scene) {

        //let device = &self.device.borrow().device;

        let data = scene.camera_data();

        self.camera_buffer.update_data(0, &data);

        // Use guaranteed unused acquire semaphore to get the index of the next frame we will render to
        // by using acquire_image
        let swap_image = unsafe {
            match self.swapchain.swapchain.acquire_image(!0, Some(&self.command.free_acquire_semaphore), None) {
                Ok((i, _)) => i as usize,
                Err(_) => {
                    panic!("Could not acquire swapchain image");
                }
            }
        };

        // Swap the acquire semaphore with the one previously associated with the image we are acquiring
        core::mem::swap(
            &mut self.command.free_acquire_semaphore,
            &mut self.command.image_acquire_semaphores[swap_image],
        );

        // Compute index into our resource ring buffers based on the frame number
        // and number of frames in flight. Pay close attention to where this index is needed
        // versus when the swapchain image index we got from acquire_image is needed.
        let frame_idx = self.command.frame % self.command.frames_in_flight;

        // Wait for the fence of the previous submission of this frame and reset it; ensures we are
        // submitting only up to maximum number of frames_in_flight if we are submitting faster than
        // the gpu can keep up with. This would also guarantee that any resources which need to be
        // updated with a CPU->GPU data copy are not in use by the GPU, so we can perform those updates.
        // In this case there are none to be done, however.
        unsafe {
            &self.device.borrow().device
                .wait_for_fence(&self.command.submission_complete_fences[frame_idx], !0)
                .expect("Failed to wait for fence");
            &self.device.borrow().device
                .reset_fence(&self.command.submission_complete_fences[frame_idx])
                .expect("Failed to reset fence");
            self.command.command_pools[frame_idx].reset();
        }

        // Rendering
        let cmd_buffer = &mut self.command.command_buffers[frame_idx];


        unsafe {

            cmd_buffer.begin(false);

            cmd_buffer.bind_compute_pipeline(&self.camera_ray_generator.pipeline);
            cmd_buffer.bind_compute_descriptor_sets(
                &self.camera_ray_generator.layout,
                0,
                vec!(
                    &self.camera_ray_generator.desc_set
                ),
                &[]
            );
            cmd_buffer.dispatch([DIMS.width/WORK_GROUP_SIZE, DIMS.height/WORK_GROUP_SIZE, 1]);


            cmd_buffer.bind_compute_pipeline(&self.vertex_skinner.pipeline);
            cmd_buffer.bind_compute_descriptor_sets(
                &self.vertex_skinner.layout,
                0,
                vec!(
                    &self.vertex_skinner.desc_set
                ),
                &[]
            );
            cmd_buffer.dispatch([scene.mesh_data.vertices.len() as u32, 1, 1]);

            let ray_barrier = memory::Barrier::Buffer{
                states: buffer::Access::SHADER_WRITE..buffer::Access::SHADER_READ,
                target: self.ray_buffer.get_buffer(),
                families: None,
                range: None..None
            };

            let vertex_barrier = memory::Barrier::Buffer{
                states: buffer::Access::SHADER_WRITE..buffer::Access::SHADER_READ,
                target: self.vertex_out_buffer.get_buffer(),
                families: None,
                range: None..None
            };

            cmd_buffer.pipeline_barrier(
                pso::PipelineStage::COMPUTE_SHADER..pso::PipelineStage::COMPUTE_SHADER,
                memory::Dependencies::empty(),
                &[vertex_barrier, ray_barrier],
            );

            cmd_buffer.bind_compute_pipeline(&self.aabb_calculator.pipeline);
            cmd_buffer.bind_compute_descriptor_sets(
                &self.aabb_calculator.layout,
                0,
                vec!(
                    &self.aabb_calculator.desc_set
                ),
                &[]
            );
            cmd_buffer.dispatch([1, 1, 1]);

            let aabb_barrier = memory::Barrier::Buffer{
                states: buffer::Access::SHADER_WRITE..buffer::Access::SHADER_READ,
                target: self.aabb_buffer.get_buffer(),
                families: None,
                range: None..None
            };

            cmd_buffer.pipeline_barrier(
                pso::PipelineStage::COMPUTE_SHADER..pso::PipelineStage::COMPUTE_SHADER,
                memory::Dependencies::empty(),
                &[aabb_barrier],
            );

            cmd_buffer.bind_compute_pipeline(&self.ray_triangle_intersector.pipeline);
            cmd_buffer.bind_compute_descriptor_sets(
                &self.ray_triangle_intersector.layout,
                0,
                vec!(
                    &self.ray_triangle_intersector.desc_set
                ),
                &[]
            );

            cmd_buffer.dispatch([DIMS.width/WORK_GROUP_SIZE, DIMS.height/WORK_GROUP_SIZE, 1]);

            let intersection_barrier = memory::Barrier::Buffer{
                states: buffer::Access::SHADER_WRITE..buffer::Access::SHADER_READ,
                target: self.intersection_buffer.get_buffer(),
                families: None,
                range: None..None
            };

            cmd_buffer.pipeline_barrier(
                pso::PipelineStage::COMPUTE_SHADER..pso::PipelineStage::COMPUTE_SHADER,
                memory::Dependencies::empty(),
                &[intersection_barrier],
            );

            cmd_buffer.bind_compute_pipeline(&self.accumulator.pipeline);
            cmd_buffer.bind_compute_descriptor_sets(
                &self.accumulator.layout,
                0,
                vec!(
                    &self.accumulator.frame_desc_sets[frame_idx],
                    &self.accumulator.desc_set
                ),
                &[]
            );

            cmd_buffer.dispatch([DIMS.width/WORK_GROUP_SIZE, DIMS.height/WORK_GROUP_SIZE, 1]);

            cmd_buffer.finish();

            let submission = Submission {
                command_buffers: Some(&*cmd_buffer),
                wait_semaphores: Some((
                    &self.command.image_acquire_semaphores[swap_image],
                    pso::PipelineStage::BOTTOM_OF_PIPE,
                )),
                signal_semaphores: Some(&self.command.submission_complete_semaphores[frame_idx]),
            };

            self.device.borrow_mut().queues.queues[0]
                .submit(submission, Some(&self.command.submission_complete_fences[frame_idx]));


            // present frame
            self.swapchain.swapchain.present(
                &mut self.device.borrow_mut().queues.queues[0],
                swap_image as gfx_hal::SwapImageIndex,
                Some(&self.command.submission_complete_semaphores[frame_idx]),
            );

        }
        // Increment our frame
        self.command.frame += 1;


    }




}