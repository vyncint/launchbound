//! Minimal CUDA driver API surface, loaded at runtime with dlopen so the
//! crate builds (and its tests run) on machines with no CUDA at all —
//! including CI and the Mac. Only the box can actually produce a timing.

#![allow(clippy::missing_safety_doc)]

use libloading::Library;
use std::ffi::c_void;

pub type CUresult = i32;
type CUdeviceptr = u64;

macro_rules! driver_api {
    ($( $name:ident : fn( $($arg:ty),* ) ; )*) => {
        // Fields carry the C symbol names verbatim.
        #[allow(non_snake_case)]
        pub struct Cuda {
            _lib: Library,
            $( $name: unsafe extern "C" fn($($arg),*) -> CUresult, )*
        }

        impl Cuda {
            /// dlopen libcuda and resolve the surface. Errors on machines
            /// without a driver — that is the honest answer there.
            pub fn load() -> Result<Self, String> {
                let lib = ["libcuda.so.1", "libcuda.so"]
                    .iter()
                    .find_map(|n| unsafe { Library::new(n).ok() })
                    .ok_or_else(|| {
                        "libcuda not found: benchmarks need an NVIDIA driver".to_string()
                    })?;
                unsafe {
                    Ok(Cuda {
                        $( $name: *lib
                            .get(concat!(stringify!($name), "\0").as_bytes())
                            .map_err(|e| format!("missing {}: {e}", stringify!($name)))?, )*
                        _lib: lib,
                    })
                }
            }
        }
    };
}

driver_api! {
    cuInit: fn(u32);
    cuDriverGetVersion: fn(*mut i32);
    cuDeviceGet: fn(*mut i32, i32);
    cuDeviceGetName: fn(*mut u8, i32, i32);
    cuDeviceGetAttribute: fn(*mut i32, i32, i32);
    cuCtxCreate_v2: fn(*mut *mut c_void, u32, i32);
    cuCtxDestroy_v2: fn(*mut c_void);
    cuCtxSynchronize: fn();
    cuModuleLoadData: fn(*mut *mut c_void, *const c_void);
    cuModuleUnload: fn(*mut c_void);
    cuModuleGetFunction: fn(*mut *mut c_void, *mut c_void, *const u8);
    cuMemAlloc_v2: fn(*mut CUdeviceptr, usize);
    cuMemFree_v2: fn(CUdeviceptr);
    cuMemcpyHtoD_v2: fn(CUdeviceptr, *const c_void, usize);
    cuMemcpyDtoH_v2: fn(*mut c_void, CUdeviceptr, usize);
    cuLaunchKernel: fn(*mut c_void, u32, u32, u32, u32, u32, u32, u32, *mut c_void, *mut *mut c_void, *mut *mut c_void);
    cuEventCreate: fn(*mut *mut c_void, u32);
    cuEventDestroy_v2: fn(*mut c_void);
    cuEventRecord: fn(*mut c_void, *mut c_void);
    cuEventSynchronize: fn(*mut c_void);
    cuEventElapsedTime: fn(*mut f32, *mut c_void, *mut c_void);
}

const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: i32 = 75;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: i32 = 76;

fn check(what: &str, code: CUresult) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{what} failed: CUresult {code}"))
    }
}

pub struct Device {
    cuda: Cuda,
    ctx: *mut c_void,
    pub name: String,
    pub cc: String,
    pub driver_version: String,
}

pub struct Module<'d> {
    device: &'d Device,
    module: *mut c_void,
    pub function: *mut c_void,
}

pub struct Buffer<'d> {
    device: &'d Device,
    pub ptr: CUdeviceptr,
    pub bytes: usize,
}

impl Device {
    pub fn open() -> Result<Self, String> {
        let cuda = Cuda::load()?;
        unsafe {
            check("cuInit", (cuda.cuInit)(0))?;
            let mut version = 0i32;
            check(
                "cuDriverGetVersion",
                (cuda.cuDriverGetVersion)(&mut version),
            )?;
            let mut dev = 0i32;
            check("cuDeviceGet", (cuda.cuDeviceGet)(&mut dev, 0))?;
            let mut name = [0u8; 128];
            check(
                "cuDeviceGetName",
                (cuda.cuDeviceGetName)(name.as_mut_ptr(), name.len() as i32, dev),
            )?;
            let (mut major, mut minor) = (0i32, 0i32);
            check(
                "cc major",
                (cuda.cuDeviceGetAttribute)(
                    &mut major,
                    CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
                    dev,
                ),
            )?;
            check(
                "cc minor",
                (cuda.cuDeviceGetAttribute)(
                    &mut minor,
                    CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR,
                    dev,
                ),
            )?;
            let mut ctx = std::ptr::null_mut();
            check("cuCtxCreate", (cuda.cuCtxCreate_v2)(&mut ctx, 0, dev))?;
            let name = String::from_utf8_lossy(
                &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())],
            )
            .to_string();
            Ok(Device {
                cuda,
                ctx,
                name,
                cc: format!("{major}.{minor}"),
                driver_version: format!("{}.{}", version / 1000, (version % 1000) / 10),
            })
        }
    }

    pub fn load_module(&self, ptx: &str, entry: &str) -> Result<Module<'_>, String> {
        let mut ptx_z = ptx.as_bytes().to_vec();
        ptx_z.push(0);
        let mut entry_z = entry.as_bytes().to_vec();
        entry_z.push(0);
        unsafe {
            let mut module = std::ptr::null_mut();
            check(
                "cuModuleLoadData",
                (self.cuda.cuModuleLoadData)(&mut module, ptx_z.as_ptr().cast()),
            )?;
            let mut function = std::ptr::null_mut();
            let got = (self.cuda.cuModuleGetFunction)(&mut function, module, entry_z.as_ptr());
            if got != 0 {
                (self.cuda.cuModuleUnload)(module);
                return Err(format!(
                    "cuModuleGetFunction({entry}) failed: CUresult {got}"
                ));
            }
            Ok(Module {
                device: self,
                module,
                function,
            })
        }
    }

    pub fn alloc(&self, bytes: usize) -> Result<Buffer<'_>, String> {
        let mut ptr = 0u64;
        unsafe { check("cuMemAlloc", (self.cuda.cuMemAlloc_v2)(&mut ptr, bytes))? };
        Ok(Buffer {
            device: self,
            ptr,
            bytes,
        })
    }

    pub fn copy_in(&self, buffer: &Buffer<'_>, data: &[u8]) -> Result<(), String> {
        assert!(data.len() <= buffer.bytes);
        unsafe {
            check(
                "cuMemcpyHtoD",
                (self.cuda.cuMemcpyHtoD_v2)(buffer.ptr, data.as_ptr().cast(), data.len()),
            )
        }
    }

    pub fn copy_out(&self, buffer: &Buffer<'_>, out: &mut [u8]) -> Result<(), String> {
        assert!(out.len() <= buffer.bytes);
        unsafe {
            check(
                "cuMemcpyDtoH",
                (self.cuda.cuMemcpyDtoH_v2)(out.as_mut_ptr().cast(), buffer.ptr, out.len()),
            )
        }
    }

    pub fn synchronize(&self) -> Result<(), String> {
        unsafe { check("cuCtxSynchronize", (self.cuda.cuCtxSynchronize)()) }
    }

    /// Launch once and return the kernel-only elapsed milliseconds,
    /// measured with a cuEvent pair on the default stream.
    pub fn timed_launch(
        &self,
        module: &Module<'_>,
        grid: [u32; 3],
        block: [u32; 3],
        params: &mut [*mut c_void],
    ) -> Result<f64, String> {
        unsafe {
            let mut ev0 = std::ptr::null_mut();
            let mut ev1 = std::ptr::null_mut();
            check("cuEventCreate", (self.cuda.cuEventCreate)(&mut ev0, 0))?;
            check("cuEventCreate", (self.cuda.cuEventCreate)(&mut ev1, 0))?;
            let stream = std::ptr::null_mut();
            check("cuEventRecord", (self.cuda.cuEventRecord)(ev0, stream))?;
            let launched = (self.cuda.cuLaunchKernel)(
                module.function,
                grid[0],
                grid[1],
                grid[2],
                block[0],
                block[1],
                block[2],
                0, // static SharedArray only: no dynamic smem
                stream,
                params.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            if launched != 0 {
                (self.cuda.cuEventDestroy_v2)(ev0);
                (self.cuda.cuEventDestroy_v2)(ev1);
                return Err(format!("cuLaunchKernel failed: CUresult {launched}"));
            }
            check("cuEventRecord", (self.cuda.cuEventRecord)(ev1, stream))?;
            check("cuEventSynchronize", (self.cuda.cuEventSynchronize)(ev1))?;
            let mut ms = 0f32;
            check(
                "cuEventElapsedTime",
                (self.cuda.cuEventElapsedTime)(&mut ms, ev0, ev1),
            )?;
            (self.cuda.cuEventDestroy_v2)(ev0);
            (self.cuda.cuEventDestroy_v2)(ev1);
            Ok(ms as f64)
        }
    }
}

impl Drop for Module<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.device.cuda.cuModuleUnload)(self.module);
        }
    }
}

impl Drop for Buffer<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.device.cuda.cuMemFree_v2)(self.ptr);
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            (self.cuda.cuCtxDestroy_v2)(self.ctx);
        }
    }
}
