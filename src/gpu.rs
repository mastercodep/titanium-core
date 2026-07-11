//! GPU-Mining über OpenCL (dynamisch geladen).
//!
//! OpenCL wird zur Laufzeit per libloading geöffnet. Ist keine OpenCL-Bibliothek
//! oder GPU vorhanden, liefert `GpuMiner::init` einfach `None` und der Miner nutzt
//! weiter die CPU. Ein Selbsttest stellt sicher, dass der GPU-Kernel exakt dieselben
//! Hashes berechnet wie der CPU-Pfad, bevor die GPU für echte Blöcke verwendet wird.

use crate::block::hash_value;
use crate::tx::sha256d;
use libloading::Library;
use std::ffi::c_void;
use std::os::raw::c_char;
use std::ptr;

// ---- OpenCL-Typen ----
type ClInt = i32;
type ClUint = u32;
type ClUlong = u64;
type ClBool = u32;
type ClDeviceType = u64;
type ClMemFlags = u64;
type Handle = *mut c_void;

const CL_SUCCESS: ClInt = 0;
const CL_TRUE: ClBool = 1;
const CL_DEVICE_TYPE_GPU: ClDeviceType = 1 << 2;
const CL_DEVICE_NAME: ClUint = 0x102B;
const CL_PROGRAM_BUILD_LOG: ClUint = 0x1183;
const CL_MEM_READ_ONLY: ClMemFlags = 1 << 2;
const CL_MEM_WRITE_ONLY: ClMemFlags = 1 << 1;
const CL_MEM_READ_WRITE: ClMemFlags = 1;
const CL_MEM_COPY_HOST_PTR: ClMemFlags = 1 << 5;

const HEADER_LEN: usize = 108;
const NONCE_OFFSET: usize = 100;

// Funktionszeiger-Typen der benötigten OpenCL-Aufrufe
type FnGetPlatformIDs = unsafe extern "C" fn(ClUint, *mut Handle, *mut ClUint) -> ClInt;
type FnGetDeviceIDs =
    unsafe extern "C" fn(Handle, ClDeviceType, ClUint, *mut Handle, *mut ClUint) -> ClInt;
type FnGetDeviceInfo =
    unsafe extern "C" fn(Handle, ClUint, usize, *mut c_void, *mut usize) -> ClInt;
type FnCreateContext = unsafe extern "C" fn(
    *const isize,
    ClUint,
    *const Handle,
    *const c_void,
    *mut c_void,
    *mut ClInt,
) -> Handle;
type FnCreateCommandQueue =
    unsafe extern "C" fn(Handle, Handle, ClUlong, *mut ClInt) -> Handle;
type FnCreateProgramWithSource =
    unsafe extern "C" fn(Handle, ClUint, *const *const c_char, *const usize, *mut ClInt) -> Handle;
type FnBuildProgram = unsafe extern "C" fn(
    Handle,
    ClUint,
    *const Handle,
    *const c_char,
    *const c_void,
    *mut c_void,
) -> ClInt;
type FnGetProgramBuildInfo =
    unsafe extern "C" fn(Handle, Handle, ClUint, usize, *mut c_void, *mut usize) -> ClInt;
type FnCreateKernel = unsafe extern "C" fn(Handle, *const c_char, *mut ClInt) -> Handle;
type FnCreateBuffer =
    unsafe extern "C" fn(Handle, ClMemFlags, usize, *mut c_void, *mut ClInt) -> Handle;
type FnSetKernelArg = unsafe extern "C" fn(Handle, ClUint, usize, *const c_void) -> ClInt;
type FnEnqueueWriteBuffer = unsafe extern "C" fn(
    Handle,
    Handle,
    ClBool,
    usize,
    usize,
    *const c_void,
    ClUint,
    *const Handle,
    *mut Handle,
) -> ClInt;
type FnEnqueueReadBuffer = unsafe extern "C" fn(
    Handle,
    Handle,
    ClBool,
    usize,
    usize,
    *mut c_void,
    ClUint,
    *const Handle,
    *mut Handle,
) -> ClInt;
type FnEnqueueNDRangeKernel = unsafe extern "C" fn(
    Handle,
    Handle,
    ClUint,
    *const usize,
    *const usize,
    *const usize,
    ClUint,
    *const Handle,
    *mut Handle,
) -> ClInt;
type FnFinish = unsafe extern "C" fn(Handle) -> ClInt;
type FnRelease = unsafe extern "C" fn(Handle) -> ClInt;

struct Cl {
    _lib: Library,
    get_platform_ids: FnGetPlatformIDs,
    get_device_ids: FnGetDeviceIDs,
    get_device_info: FnGetDeviceInfo,
    create_context: FnCreateContext,
    create_command_queue: FnCreateCommandQueue,
    create_program_with_source: FnCreateProgramWithSource,
    build_program: FnBuildProgram,
    get_program_build_info: FnGetProgramBuildInfo,
    create_kernel: FnCreateKernel,
    create_buffer: FnCreateBuffer,
    set_kernel_arg: FnSetKernelArg,
    enqueue_write_buffer: FnEnqueueWriteBuffer,
    enqueue_read_buffer: FnEnqueueReadBuffer,
    enqueue_nd_range_kernel: FnEnqueueNDRangeKernel,
    finish: FnFinish,
    release_mem: FnRelease,
    release_kernel: FnRelease,
    release_program: FnRelease,
    release_queue: FnRelease,
    release_context: FnRelease,
}

unsafe fn load_symbol<T: Copy>(lib: &Library, name: &[u8]) -> Option<T> {
    let sym: libloading::Symbol<T> = lib.get(name).ok()?;
    Some(*sym)
}

impl Cl {
    fn load() -> Option<Cl> {
        let candidates: &[&str] = if cfg!(target_os = "windows") {
            &["OpenCL.dll"]
        } else if cfg!(target_os = "macos") {
            &["/System/Library/Frameworks/OpenCL.framework/OpenCL"]
        } else {
            &["libOpenCL.so.1", "libOpenCL.so"]
        };
        let lib = candidates
            .iter()
            .find_map(|c| unsafe { Library::new(c).ok() })?;
        unsafe {
            Some(Cl {
                get_platform_ids: load_symbol(&lib, b"clGetPlatformIDs\0")?,
                get_device_ids: load_symbol(&lib, b"clGetDeviceIDs\0")?,
                get_device_info: load_symbol(&lib, b"clGetDeviceInfo\0")?,
                create_context: load_symbol(&lib, b"clCreateContext\0")?,
                create_command_queue: load_symbol(&lib, b"clCreateCommandQueue\0")?,
                create_program_with_source: load_symbol(&lib, b"clCreateProgramWithSource\0")?,
                build_program: load_symbol(&lib, b"clBuildProgram\0")?,
                get_program_build_info: load_symbol(&lib, b"clGetProgramBuildInfo\0")?,
                create_kernel: load_symbol(&lib, b"clCreateKernel\0")?,
                create_buffer: load_symbol(&lib, b"clCreateBuffer\0")?,
                set_kernel_arg: load_symbol(&lib, b"clSetKernelArg\0")?,
                enqueue_write_buffer: load_symbol(&lib, b"clEnqueueWriteBuffer\0")?,
                enqueue_read_buffer: load_symbol(&lib, b"clEnqueueReadBuffer\0")?,
                enqueue_nd_range_kernel: load_symbol(&lib, b"clEnqueueNDRangeKernel\0")?,
                finish: load_symbol(&lib, b"clFinish\0")?,
                release_mem: load_symbol(&lib, b"clReleaseMemObject\0")?,
                release_kernel: load_symbol(&lib, b"clReleaseKernel\0")?,
                release_program: load_symbol(&lib, b"clReleaseProgram\0")?,
                release_queue: load_symbol(&lib, b"clReleaseCommandQueue\0")?,
                release_context: load_symbol(&lib, b"clReleaseContext\0")?,
                _lib: lib,
            })
        }
    }
}

const KERNEL_SRC: &str = r#"
__constant uint K[64] = {
0x428a2f98u,0x71374491u,0xb5c0fbcfu,0xe9b5dba5u,0x3956c25bu,0x59f111f1u,0x923f82a4u,0xab1c5ed5u,
0xd807aa98u,0x12835b01u,0x243185beu,0x550c7dc3u,0x72be5d74u,0x80deb1feu,0x9bdc06a7u,0xc19bf174u,
0xe49b69c1u,0xefbe4786u,0x0fc19dc6u,0x240ca1ccu,0x2de92c6fu,0x4a7484aau,0x5cb0a9dcu,0x76f988dau,
0x983e5152u,0xa831c66du,0xb00327c8u,0xbf597fc7u,0xc6e00bf3u,0xd5a79147u,0x06ca6351u,0x14292967u,
0x27b70a85u,0x2e1b2138u,0x4d2c6dfcu,0x53380d13u,0x650a7354u,0x766a0abbu,0x81c2c92eu,0x92722c85u,
0xa2bfe8a1u,0xa81a664bu,0xc24b8b70u,0xc76c51a3u,0xd192e819u,0xd6990624u,0xf40e3585u,0x106aa070u,
0x19a4c116u,0x1e376c08u,0x2748774cu,0x34b0bcb5u,0x391c0cb3u,0x4ed8aa4au,0x5b9cca4fu,0x682e6ff3u,
0x748f82eeu,0x78a5636fu,0x84c87814u,0x8cc70208u,0x90befffau,0xa4506cebu,0xbef9a3f7u,0xc67178f2u};

inline uint rotr(uint x, uint n){ return (x >> n) | (x << (32u - n)); }

void sha256_transform(uint* h, const uchar* p){
    uint w[64];
    for(int i=0;i<16;i++){
        w[i]=((uint)p[i*4]<<24)|((uint)p[i*4+1]<<16)|((uint)p[i*4+2]<<8)|((uint)p[i*4+3]);
    }
    for(int i=16;i<64;i++){
        uint s0=rotr(w[i-15],7)^rotr(w[i-15],18)^(w[i-15]>>3);
        uint s1=rotr(w[i-2],17)^rotr(w[i-2],19)^(w[i-2]>>10);
        w[i]=w[i-16]+s0+w[i-7]+s1;
    }
    uint a=h[0],b=h[1],c=h[2],d=h[3],e=h[4],f=h[5],g=h[6],hh=h[7];
    for(int i=0;i<64;i++){
        uint S1=rotr(e,6)^rotr(e,11)^rotr(e,25);
        uint ch=(e&f)^((~e)&g);
        uint t1=hh+S1+ch+K[i]+w[i];
        uint S0=rotr(a,2)^rotr(a,13)^rotr(a,22);
        uint maj=(a&b)^(a&c)^(b&c);
        uint t2=S0+maj;
        hh=g; g=f; f=e; e=d+t1; d=c; c=b; b=a; a=t1+t2;
    }
    h[0]+=a;h[1]+=b;h[2]+=c;h[3]+=d;h[4]+=e;h[5]+=f;h[6]+=g;h[7]+=hh;
}

void init_h(uint* h){
    h[0]=0x6a09e667u;h[1]=0xbb67ae85u;h[2]=0x3c6ef372u;h[3]=0xa54ff53au;
    h[4]=0x510e527fu;h[5]=0x9b05688cu;h[6]=0x1f83d9abu;h[7]=0x5be0cd19u;
}

// Berechnet SHA256d des 108-Byte-Headers mit gesetztem nonce; liefert die
// oberen 128 Bit als (hi, lo).
void hash_header(const uchar* header, ulong nonce, ulong* hi, ulong* lo){
    uchar block[128];
    for(int i=0;i<108;i++) block[i]=header[i];
    for(int i=0;i<8;i++) block[100+i]=(uchar)((nonce >> (8*i)) & 0xffu);
    block[108]=0x80u;
    for(int i=109;i<128;i++) block[i]=0u;
    ulong bits=108u*8u;
    for(int i=0;i<8;i++) block[127-i]=(uchar)((bits >> (8*i)) & 0xffu);
    uint h[8]; init_h(h);
    sha256_transform(h, block);
    sha256_transform(h, block+64);
    uchar mid[64];
    for(int i=0;i<8;i++){ mid[i*4]=(uchar)(h[i]>>24); mid[i*4+1]=(uchar)(h[i]>>16); mid[i*4+2]=(uchar)(h[i]>>8); mid[i*4+3]=(uchar)h[i]; }
    mid[32]=0x80u;
    for(int i=33;i<64;i++) mid[i]=0u;
    ulong bits2=32u*8u;
    for(int i=0;i<8;i++) mid[63-i]=(uchar)((bits2 >> (8*i)) & 0xffu);
    uint h2[8]; init_h(h2);
    sha256_transform(h2, mid);
    *hi = ((ulong)h2[0]<<32) | (ulong)h2[1];
    *lo = ((ulong)h2[2]<<32) | (ulong)h2[3];
}

__kernel void mine(__global const uchar* header, const ulong base_nonce,
                   const ulong target_hi, const ulong target_lo,
                   __global volatile uint* result){
    ulong nonce = base_nonce + (ulong)get_global_id(0);
    ulong hi, lo;
    hash_header(header, nonce, &hi, &lo);
    bool meets = (hi < target_hi) || (hi==target_hi && lo <= target_lo);
    if(meets){
        if(atomic_cmpxchg(&result[0], 0u, 1u) == 0u){
            result[1]=(uint)(nonce & 0xffffffffu);
            result[2]=(uint)(nonce >> 32);
        }
    }
}

// Nur für den Selbsttest: gibt hi/lo für base_nonce (global id 0) zurück.
__kernel void hash_at(__global const uchar* header, const ulong base_nonce,
                      __global ulong* out){
    ulong hi, lo;
    hash_header(header, base_nonce, &hi, &lo);
    out[0]=hi; out[1]=lo;
}
"#;

pub struct GpuMiner {
    cl: Cl,
    context: Handle,
    queue: Handle,
    program: Handle,
    kernel_mine: Handle,
    kernel_hash: Handle,
    header_buf: Handle,
    result_buf: Handle,
    out_buf: Handle,
    pub device_name: String,
    /// Anzahl Work-Items pro Kernel-Aufruf
    pub batch: u64,
}

// OpenCL-Handles sind über Threads teilbar; wir serialisieren die Nutzung selbst.
unsafe impl Send for GpuMiner {}

impl GpuMiner {
    pub fn init() -> Option<GpuMiner> {
        let cl = Cl::load()?;
        unsafe {
            let mut platforms = [ptr::null_mut::<c_void>(); 8];
            let mut num_pf: ClUint = 0;
            if (cl.get_platform_ids)(8, platforms.as_mut_ptr(), &mut num_pf) != CL_SUCCESS
                || num_pf == 0
            {
                return None;
            }
            // Erste GPU über alle Plattformen finden
            let mut device: Handle = ptr::null_mut();
            for &pf in platforms.iter().take(num_pf as usize) {
                let mut dev = [ptr::null_mut::<c_void>(); 4];
                let mut num_dev: ClUint = 0;
                if (cl.get_device_ids)(pf, CL_DEVICE_TYPE_GPU, 4, dev.as_mut_ptr(), &mut num_dev)
                    == CL_SUCCESS
                    && num_dev > 0
                {
                    device = dev[0];
                    break;
                }
            }
            if device.is_null() {
                return None;
            }

            // Gerätename
            let mut name_buf = [0u8; 256];
            let mut name_len: usize = 0;
            (cl.get_device_info)(
                device,
                CL_DEVICE_NAME,
                256,
                name_buf.as_mut_ptr() as *mut c_void,
                &mut name_len,
            );
            let device_name = String::from_utf8_lossy(&name_buf[..name_len.saturating_sub(1)])
                .trim()
                .to_string();

            let mut err: ClInt = 0;
            let context = (cl.create_context)(
                ptr::null(),
                1,
                &device,
                ptr::null(),
                ptr::null_mut(),
                &mut err,
            );
            if err != CL_SUCCESS || context.is_null() {
                return None;
            }
            let queue = (cl.create_command_queue)(context, device, 0, &mut err);
            if err != CL_SUCCESS {
                (cl.release_context)(context);
                return None;
            }

            let src = KERNEL_SRC.as_ptr() as *const c_char;
            let src_len = KERNEL_SRC.len();
            let program =
                (cl.create_program_with_source)(context, 1, &src, &src_len, &mut err);
            if err != CL_SUCCESS {
                (cl.release_queue)(queue);
                (cl.release_context)(context);
                return None;
            }
            let build_res =
                (cl.build_program)(program, 1, &device, ptr::null(), ptr::null(), ptr::null_mut());
            if build_res != CL_SUCCESS {
                // Build-Log holen (Diagnose)
                let mut log = [0u8; 2048];
                let mut log_len: usize = 0;
                (cl.get_program_build_info)(
                    program,
                    device,
                    CL_PROGRAM_BUILD_LOG,
                    2048,
                    log.as_mut_ptr() as *mut c_void,
                    &mut log_len,
                );
                eprintln!(
                    "OpenCL-Build fehlgeschlagen: {}",
                    String::from_utf8_lossy(&log[..log_len.min(2048)])
                );
                (cl.release_program)(program);
                (cl.release_queue)(queue);
                (cl.release_context)(context);
                return None;
            }

            let kmine = (cl.create_kernel)(program, b"mine\0".as_ptr() as *const c_char, &mut err);
            let khash =
                (cl.create_kernel)(program, b"hash_at\0".as_ptr() as *const c_char, &mut err);
            if err != CL_SUCCESS || kmine.is_null() || khash.is_null() {
                (cl.release_program)(program);
                (cl.release_queue)(queue);
                (cl.release_context)(context);
                return None;
            }

            let header_buf =
                (cl.create_buffer)(context, CL_MEM_READ_ONLY, HEADER_LEN, ptr::null_mut(), &mut err);
            let result_buf = (cl.create_buffer)(
                context,
                CL_MEM_READ_WRITE,
                12, // 3 * u32
                ptr::null_mut(),
                &mut err,
            );
            let out_buf =
                (cl.create_buffer)(context, CL_MEM_WRITE_ONLY, 16, ptr::null_mut(), &mut err);
            if header_buf.is_null() || result_buf.is_null() || out_buf.is_null() {
                return None;
            }

            let mut g = GpuMiner {
                cl,
                context,
                queue,
                program,
                kernel_mine: kmine,
                kernel_hash: khash,
                header_buf,
                result_buf,
                out_buf,
                device_name,
                batch: 1 << 21, // ~2 Mio. Hashes pro Aufruf
            };
            if !g.self_test() {
                eprintln!("GPU-Selbsttest fehlgeschlagen - nutze CPU.");
                return None;
            }
            Some(g)
        }
    }

    /// Vergleicht GPU-Hash mit CPU-Hash für einen festen Header/Nonce.
    fn self_test(&mut self) -> bool {
        // Beispielhafter 108-Byte-Header
        let mut header = [0u8; HEADER_LEN];
        for (i, b) in header.iter_mut().enumerate() {
            *b = (i * 7 + 13) as u8;
        }
        let test_nonce: u64 = 0x0123_4567_89ab_cdef;

        // CPU-Referenz
        let mut cpu = header;
        cpu[NONCE_OFFSET..].copy_from_slice(&test_nonce.to_le_bytes());
        let cpu_hash = sha256d(&cpu);
        let cpu_val = hash_value(&cpu_hash);
        let cpu_hi = (cpu_val >> 64) as u64;
        let cpu_lo = cpu_val as u64;

        unsafe {
            let cl = &self.cl;
            if (cl.enqueue_write_buffer)(
                self.queue,
                self.header_buf,
                CL_TRUE,
                0,
                HEADER_LEN,
                header.as_ptr() as *const c_void,
                0,
                ptr::null(),
                ptr::null_mut(),
            ) != CL_SUCCESS
            {
                return false;
            }
            (cl.set_kernel_arg)(
                self.kernel_hash,
                0,
                std::mem::size_of::<Handle>(),
                &self.header_buf as *const _ as *const c_void,
            );
            (cl.set_kernel_arg)(
                self.kernel_hash,
                1,
                std::mem::size_of::<ClUlong>(),
                &test_nonce as *const _ as *const c_void,
            );
            (cl.set_kernel_arg)(
                self.kernel_hash,
                2,
                std::mem::size_of::<Handle>(),
                &self.out_buf as *const _ as *const c_void,
            );
            let global: usize = 1;
            if (cl.enqueue_nd_range_kernel)(
                self.queue,
                self.kernel_hash,
                1,
                ptr::null(),
                &global,
                ptr::null(),
                0,
                ptr::null(),
                ptr::null_mut(),
            ) != CL_SUCCESS
            {
                return false;
            }
            (cl.finish)(self.queue);
            let mut out = [0u64; 2];
            if (cl.enqueue_read_buffer)(
                self.queue,
                self.out_buf,
                CL_TRUE,
                0,
                16,
                out.as_mut_ptr() as *mut c_void,
                0,
                ptr::null(),
                ptr::null_mut(),
            ) != CL_SUCCESS
            {
                return false;
            }
            out[0] == cpu_hi && out[1] == cpu_lo
        }
    }

    /// Durchsucht [base_nonce, base_nonce + batch) auf der GPU.
    /// Gibt den Gewinner-Nonce zurück, falls einer das Target erfüllt.
    pub fn search(&self, header: &[u8; HEADER_LEN], target: u128, base_nonce: u64) -> Option<u64> {
        let target_hi = (target >> 64) as u64;
        let target_lo = target as u64;
        unsafe {
            let cl = &self.cl;
            let zero = [0u32; 3];
            (cl.enqueue_write_buffer)(
                self.queue,
                self.result_buf,
                CL_TRUE,
                0,
                12,
                zero.as_ptr() as *const c_void,
                0,
                ptr::null(),
                ptr::null_mut(),
            );
            (cl.enqueue_write_buffer)(
                self.queue,
                self.header_buf,
                CL_TRUE,
                0,
                HEADER_LEN,
                header.as_ptr() as *const c_void,
                0,
                ptr::null(),
                ptr::null_mut(),
            );
            (cl.set_kernel_arg)(
                self.kernel_mine,
                0,
                std::mem::size_of::<Handle>(),
                &self.header_buf as *const _ as *const c_void,
            );
            (cl.set_kernel_arg)(
                self.kernel_mine,
                1,
                std::mem::size_of::<ClUlong>(),
                &base_nonce as *const _ as *const c_void,
            );
            (cl.set_kernel_arg)(
                self.kernel_mine,
                2,
                std::mem::size_of::<ClUlong>(),
                &target_hi as *const _ as *const c_void,
            );
            (cl.set_kernel_arg)(
                self.kernel_mine,
                3,
                std::mem::size_of::<ClUlong>(),
                &target_lo as *const _ as *const c_void,
            );
            (cl.set_kernel_arg)(
                self.kernel_mine,
                4,
                std::mem::size_of::<Handle>(),
                &self.result_buf as *const _ as *const c_void,
            );
            let global = self.batch as usize;
            if (cl.enqueue_nd_range_kernel)(
                self.queue,
                self.kernel_mine,
                1,
                ptr::null(),
                &global,
                ptr::null(),
                0,
                ptr::null(),
                ptr::null_mut(),
            ) != CL_SUCCESS
            {
                return None;
            }
            (cl.finish)(self.queue);
            let mut result = [0u32; 3];
            (cl.enqueue_read_buffer)(
                self.queue,
                self.result_buf,
                CL_TRUE,
                0,
                12,
                result.as_mut_ptr() as *mut c_void,
                0,
                ptr::null(),
                ptr::null_mut(),
            );
            if result[0] == 1 {
                Some((result[1] as u64) | ((result[2] as u64) << 32))
            } else {
                None
            }
        }
    }
}

impl Drop for GpuMiner {
    fn drop(&mut self) {
        unsafe {
            let cl = &self.cl;
            (cl.release_mem)(self.header_buf);
            (cl.release_mem)(self.result_buf);
            (cl.release_mem)(self.out_buf);
            (cl.release_kernel)(self.kernel_mine);
            (cl.release_kernel)(self.kernel_hash);
            (cl.release_program)(self.program);
            (cl.release_queue)(self.queue);
            (cl.release_context)(self.context);
        }
    }
}
