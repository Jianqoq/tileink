//! Device selection, creation and immutable rendering limits.
use super::*;
use crate::native::runtime::renderer::recording::Limits;

impl Dx12 {
    #[cfg(test)]
    pub fn new(identity: &str) -> Result<Self> {
        // Isolated verification creates native debug devices before wgpu devices.
        // Subsequent calls are no-ops, including when foreign devices now exist.
        unsafe {
            debug::enable_validation()?;
        }
        Self::with_options(&crate::native::NativeContextOptions {
            physical_adapter: Some(identity.to_owned()),
            validation: true,
        })
    }

    pub fn with_options(options: &crate::native::NativeContextOptions) -> Result<Self> {
        unsafe {
            let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let mut selected = None;
            for index in 0.. {
                let adapter = match factory.EnumAdapters1(index) {
                    Ok(a) => a,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(e.into()),
                };
                let desc = adapter.GetDesc1()?;
                let bytes = [
                    desc.AdapterLuid.LowPart.to_le_bytes(),
                    desc.AdapterLuid.HighPart.to_le_bytes(),
                ]
                .concat();
                let actual: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                if options
                    .physical_adapter
                    .as_ref()
                    .is_none_or(|identity| identity == &actual)
                    && (options.physical_adapter.is_some()
                        || desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0)
                {
                    selected = Some((adapter, actual));
                    break;
                }
            }
            let (adapter, identity) =
                selected.ok_or("requested native DX12 physical GPU unavailable")?;
            let device = debug::create_device(&adapter)?;
            let queue: ID3D12CommandQueue =
                device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                    ..Default::default()
                })?;
            let messages = Validation::new(if options.validation {
                Some(device.cast().map_err(|error| format!(
                    "DX12 validation requires a debug layer enabled before device creation: {error}"
                ))?)
            } else {
                None
            });
            let cache_identity = compute_pipeline::identity(&adapter, &identity)?;
            let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let event = CreateEventW(None, false, false, None)?;
            Ok(Self {
                gpu: GpuOwners {
                    device,
                    adapter,
                    physical_identity: identity,
                    messages,
                    queue,
                    signature: None,
                    pipelines: BTreeMap::new(),
                    fence,
                    pending: Pending::new(),
                    compute_pipelines: BTreeMap::new(),
                    cache_identity,
                },
                event,
                retirement: Retirement::Idle,
                #[cfg(test)]
                inject_signal_failure: false,
            })
        }
    }

    pub fn limits(&self) -> Limits {
        Limits {
            image_dimension: D3D12_REQ_TEXTURE2D_U_OR_V_DIMENSION,
            atlas_pages: D3D12_REQ_TEXTURE2D_ARRAY_AXIS_DIMENSION,
            texture_table_len: crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY,
            dispatch_dimension: D3D12_CS_DISPATCH_MAX_THREAD_GROUPS_PER_DIMENSION,
        }
    }
}
