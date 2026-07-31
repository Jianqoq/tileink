use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

const TOTAL_WORDS: usize = 1024 * 1024;
const DIRTY_WORDS: usize = 64 * 1024;
const RANGE_COUNTS: [usize; 5] = [1, 4, 16, 64, 256];

const SCATTER_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> upload: array<u32>;
@group(0) @binding(1) var<storage, read_write> destination: array<u32>;

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) workgroup: vec3<u32>,
    @builtin(local_invocation_id) local: vec3<u32>,
) {
    let descriptor = 4u + workgroup.x * 4u;
    let payload_base = upload[0];
    let dst = upload[descriptor];
    let src = upload[descriptor + 1u];
    let len = upload[descriptor + 2u];
    var offset = local.x;
    loop {
        if offset >= len {
            break;
        }
        destination[dst + offset] = upload[payload_base + src + offset];
        offset += 256u;
    }
}
"#;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    destination: wgpu::Buffer,
    upload: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
}

impl Gpu {
    fn new(max_upload_words: usize) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("request upload experiment adapter");
        eprintln!("range scatter benchmark adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tileink range scatter benchmark device"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .expect("request upload experiment device");
        let destination = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("range scatter benchmark destination"),
            size: (TOTAL_WORDS * size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let upload = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("range scatter benchmark packed input"),
            size: (max_upload_words * size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("range scatter benchmark shader"),
            source: wgpu::ShaderSource::Wgsl(SCATTER_SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("range scatter benchmark pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("range scatter benchmark bind group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: upload.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: destination.as_entire_binding(),
                },
            ],
        });
        Self {
            device,
            queue,
            destination,
            upload,
            pipeline,
            bind_group,
        }
    }

    fn submit_direct(&self, data: &[u32], ranges: &[std::ops::Range<usize>]) {
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("range scatter benchmark direct encoder"),
            });
        for range in ranges {
            self.queue.write_buffer(
                &self.destination,
                (range.start * size_of::<u32>()) as u64,
                bytemuck::cast_slice(&data[range.clone()]),
            );
        }
        self.queue.submit([encoder.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for direct upload");
    }

    fn submit_scatter(&self, packed: &[u32], range_count: u32) {
        self.queue
            .write_buffer(&self.upload, 0, bytemuck::cast_slice(packed));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("range scatter benchmark compute encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("range scatter benchmark compute pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(range_count, 1, 1);
        }
        self.queue.submit([encoder.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for scatter upload");
    }

    fn submit_packed_copies(&self, packed: &[u32], ranges: &[std::ops::Range<usize>]) {
        self.queue
            .write_buffer(&self.upload, 0, bytemuck::cast_slice(packed));
        let payload_base = packed[0] as usize;
        let mut source = payload_base;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("range scatter benchmark copy encoder"),
            });
        for range in ranges {
            encoder.copy_buffer_to_buffer(
                &self.upload,
                (source * size_of::<u32>()) as u64,
                &self.destination,
                (range.start * size_of::<u32>()) as u64,
                (range.len() * size_of::<u32>()) as u64,
            );
            source += range.len();
        }
        self.queue.submit([encoder.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for packed copy upload");
    }
}

fn ranges(count: usize) -> Vec<std::ops::Range<usize>> {
    assert_eq!(DIRTY_WORDS % count, 0);
    let words = DIRTY_WORDS / count;
    (0..count)
        .map(|index| {
            let start = index * TOTAL_WORDS / count;
            start..start + words
        })
        .collect()
}

fn pack(data: &[u32], ranges: &[std::ops::Range<usize>]) -> Vec<u32> {
    let payload_base = 4 + ranges.len() * 4;
    let mut packed = vec![0; payload_base];
    packed[0] = payload_base as u32;
    packed[1] = ranges.len() as u32;
    let mut payload_offset = 0;
    for (index, range) in ranges.iter().enumerate() {
        let descriptor = 4 + index * 4;
        packed[descriptor] = range.start as u32;
        packed[descriptor + 1] = payload_offset as u32;
        packed[descriptor + 2] = range.len() as u32;
        packed.extend_from_slice(&data[range.clone()]);
        payload_offset += range.len();
    }
    packed
}

fn range_scatter(c: &mut Criterion) {
    let data = (0..TOTAL_WORDS as u32).collect::<Vec<_>>();
    let max_upload_words = 4 + RANGE_COUNTS[RANGE_COUNTS.len() - 1] * 4 + DIRTY_WORDS;
    let gpu = Gpu::new(max_upload_words);
    let mut group = c.benchmark_group("upload_ranges_256k_e2e");
    group.throughput(Throughput::Bytes((DIRTY_WORDS * size_of::<u32>()) as u64));
    for count in RANGE_COUNTS {
        let ranges = ranges(count);
        let packed = pack(&data, &ranges);
        group.bench_with_input(
            BenchmarkId::new("direct_write_buffer", count),
            &count,
            |b, _| {
                b.iter(|| gpu.submit_direct(&data, &ranges));
            },
        );
        group.bench_with_input(BenchmarkId::new("packed_scatter", count), &count, |b, _| {
            b.iter(|| gpu.submit_scatter(&packed, count as u32));
        });
        group.bench_with_input(BenchmarkId::new("packed_copies", count), &count, |b, _| {
            b.iter(|| gpu.submit_packed_copies(&packed, &ranges));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(1));
    targets = range_scatter
}
criterion_main!(benches);
