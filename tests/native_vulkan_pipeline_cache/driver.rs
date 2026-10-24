//! Controlled native-call results and handle accounting; no Vulkan loader or GPU.
use ash::vk::{self, Handle};
use std::{cell::RefCell, collections::BTreeSet, ffi::c_void, path::PathBuf};

pub struct Driver {
    pub calls: Vec<&'static str>,
    pub inputs: Vec<Vec<u8>>,
    pub bytes: Vec<u8>,
    pub fail: Option<(&'static str, vk::Result)>,
    pub partial: bool,
    pub block_publication: Option<PathBuf>,
    pub caches: BTreeSet<u64>,
    pub pipelines: BTreeSet<u64>,
    pub invalid_destroy: bool,
    next: u64,
}
impl Driver {
    fn new() -> Self {
        let mut bytes: Vec<_> = [32u32, 1, 23, 41]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        bytes.extend([17; 16]);
        Self {
            calls: vec![],
            inputs: vec![],
            bytes,
            fail: None,
            partial: false,
            block_publication: None,
            caches: BTreeSet::new(),
            pipelines: BTreeSet::new(),
            invalid_destroy: false,
            next: 1,
        }
    }
    pub fn reset_calls(&mut self) {
        self.calls.clear();
        self.inputs.clear();
    }
    fn call(&mut self, name: &'static str) -> Option<vk::Result> {
        self.calls.push(name);
        if self.fail.is_some_and(|(operation, _)| operation == name) {
            self.fail.take().map(|(_, error)| error)
        } else {
            None
        }
    }
    const fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}
thread_local! { static DRIVER: RefCell<Driver> = RefCell::new(Driver::new()); }
pub fn with<T>(run: impl FnOnce(&mut Driver) -> T) -> T {
    DRIVER.with_borrow_mut(run)
}

pub fn device() -> ash::Device {
    // SAFETY: Each loaded function has the Vulkan ABI/signature and uses only the thread-local fake driver state.
    unsafe {
        ash::Device::load_with(
            |name| match name.to_bytes() {
                b"vkCreatePipelineCache" => create_cache as *const c_void,
                b"vkCreateComputePipelines" => create_pipelines as *const c_void,
                b"vkGetPipelineCacheData" => cache_data as *const c_void,
                b"vkDestroyPipelineCache" => destroy_cache as *const c_void,
                b"vkDestroyPipeline" => destroy_pipeline as *const c_void,
                _ => std::ptr::null(),
            },
            vk::Device::from_raw(1),
        )
    }
}
unsafe extern "system" fn create_cache(
    _: vk::Device,
    info: *const vk::PipelineCacheCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::PipelineCache,
) -> vk::Result {
    with(|d| {
        // SAFETY: The wrapper supplies a live create-info pointer for this synchronous call.
        let info = unsafe { &*info };
        d.inputs.push(if info.initial_data_size == 0 {
            vec![]
        } else {
            // SAFETY: The create-info bytes remain live for initial_data_size bytes and the destination Vec owns its copy.
            unsafe {
                std::slice::from_raw_parts(info.p_initial_data.cast::<u8>(), info.initial_data_size)
                    .to_vec()
            }
        });
        if let Some(error) = d.call("cache") {
            return error;
        }
        let id = d.id();
        d.caches.insert(id);
        // SAFETY: The Vulkan wrapper supplies writable storage for one pipeline cache handle.
        unsafe {
            output.write(vk::PipelineCache::from_raw(id));
        }
        vk::Result::SUCCESS
    })
}
unsafe extern "system" fn create_pipelines(
    _: vk::Device,
    _: vk::PipelineCache,
    count: u32,
    _: *const vk::ComputePipelineCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::Pipeline,
) -> vk::Result {
    with(|d| {
        let error = d.call("pipeline");
        for index in 0..count {
            let pipeline = if error.is_none() || d.partial {
                let id = d.id();
                d.pipelines.insert(id);
                vk::Pipeline::from_raw(id)
            } else {
                vk::Pipeline::null()
            };
            // SAFETY: The wrapper allocated count writable pipeline slots; each index is strictly below count.
            unsafe {
                output.add(index as usize).write(pipeline);
            }
        }
        error.unwrap_or(vk::Result::SUCCESS)
    })
}
unsafe extern "system" fn cache_data(
    _: vk::Device,
    _: vk::PipelineCache,
    size: *mut usize,
    data: *mut c_void,
) -> vk::Result {
    with(|d| {
        if let Some(error) = d.call("data") {
            return error;
        }
        if !data.is_null() {
            // SAFETY: The fake query reports this exact byte length before the wrapper allocates the destination buffer.
            unsafe {
                std::ptr::copy_nonoverlapping(d.bytes.as_ptr(), data.cast(), d.bytes.len());
            }
            if let Some(root) = d.block_publication.take() {
                let block = || -> std::io::Result<()> {
                    let lock = std::fs::read_dir(root)?
                        .map(|p| p.map(|p| p.path()))
                        .collect::<std::io::Result<Vec<_>>>()?
                        .into_iter()
                        .find(|p| p.extension().is_some_and(|e| e == "lock"))
                        .unwrap();
                    std::fs::create_dir(lock.with_extension("shader"))
                };
                if block().is_err() {
                    return vk::Result::ERROR_UNKNOWN;
                }
            }
        }
        // SAFETY: The wrapper supplies a writable size pointer throughout this query.
        unsafe {
            size.write(d.bytes.len());
        }
        vk::Result::SUCCESS
    })
}
unsafe extern "system" fn destroy_cache(
    _: vk::Device,
    cache: vk::PipelineCache,
    _: *const vk::AllocationCallbacks<'_>,
) {
    if cache != vk::PipelineCache::null() {
        with(|d| d.invalid_destroy |= !d.caches.remove(&cache.as_raw()));
    }
}
unsafe extern "system" fn destroy_pipeline(
    _: vk::Device,
    pipeline: vk::Pipeline,
    _: *const vk::AllocationCallbacks<'_>,
) {
    if pipeline != vk::Pipeline::null() {
        with(|d| d.invalid_destroy |= !d.pipelines.remove(&pipeline.as_raw()));
    }
}
