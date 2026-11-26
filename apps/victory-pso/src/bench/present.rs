//! A swap chain in a small window of our own, cleared and presented.

use anyhow::{Context, Result, ensure};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HRESULT, Interface, w};

use super::gpu::{self, qpc, us};
use super::{Bench, Row};

const BUFFERS: usize = 3;
/// `Present`'s success code for a window nothing of is visible.
const OCCLUDED: HRESULT = HRESULT(0x087A_0001);

unsafe extern "system" fn proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: the arguments the system called with.
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}

/// Takes what the system sent the window, which would otherwise count it as
/// hung.
fn pump() {
    let mut message = MSG::default();
    // SAFETY: this thread's own queue.
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

struct Window(HWND);

impl Window {
    /// Shown without taking the focus from what the user is doing.
    fn new() -> Result<Window> {
        // SAFETY: a class with a procedure of this module.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("victory-pso bench");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE,
                class,
                w!("victory-pso bench"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                640,
                360,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Ok(Window(hwnd))
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: a window of this thread.
        let _ = unsafe { DestroyWindow(self.0) };
        pump();
    }
}

pub fn run(b: &mut Bench) -> Result<()> {
    let device = b.adapter.device.clone();
    // SAFETY: plain DXGI and D3D12 calls with live objects; the buffers are
    // released only once the queue is idle.
    unsafe {
        let factory: IDXGIFactory5 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let mut tearing = BOOL(0);
        let tearing = factory
            .CheckFeatureSupport(
                DXGI_FEATURE_PRESENT_ALLOW_TEARING,
                (&raw mut tearing).cast(),
                size_of_val(&tearing) as u32,
            )
            .is_ok()
            && tearing.as_bool();
        let window = Window::new()?;
        pump();
        let flags = if tearing {
            DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32
        } else {
            0
        };
        let chain: IDXGISwapChain3 = factory
            .CreateSwapChainForHwnd(
                &b.direct.queue,
                window.0,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: 640,
                    Height: 360,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: BUFFERS as u32,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                    Flags: flags,
                    ..Default::default()
                },
                None,
                None::<&IDXGIOutput>,
            )?
            .cast()?;
        factory.MakeWindowAssociation(window.0, DXGI_MWA_NO_ALT_ENTER)?;
        let targets: ID3D12DescriptorHeap =
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: BUFFERS as u32,
                ..Default::default()
            })?;
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize;
        let mut frames = Vec::new();
        for n in 0..BUFFERS {
            let picture: ID3D12Resource = chain.GetBuffer(n as u32)?;
            let mut at = targets.GetCPUDescriptorHandleForHeapStart();
            at.ptr += n * step;
            device.CreateRenderTargetView(&picture, None, at);
            let allocator: ID3D12CommandAllocator =
                device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None::<&ID3D12PipelineState>,
            )?;
            list.Close()?;
            frames.push((picture, at, allocator, list, 0u64));
        }
        let mut modes = vec![
            ("waiting for the display", 1u32, DXGI_PRESENT(0)),
            ("not waiting", 0, DXGI_PRESENT(0)),
        ];
        if tearing {
            modes.push(("tearing allowed", 0, DXGI_PRESENT_ALLOW_TEARING));
        }
        let count = if b.quick { 8 } else { 180 };
        for (name, interval, flags) in modes {
            let (mut whole, mut call, mut occluded) = (Vec::new(), Vec::new(), 0);
            let mut before = None;
            for frame in 0..count + 2 {
                pump();
                let now = chain.GetCurrentBackBufferIndex() as usize;
                let (picture, at, allocator, list, free) = &mut frames[now];
                b.direct.wait(*free)?;
                allocator.Reset()?;
                list.Reset(&*allocator, None::<&ID3D12PipelineState>)?;
                let (shown, drawn) = (
                    D3D12_RESOURCE_STATE_PRESENT,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                );
                list.ResourceBarrier(&[gpu::transition(picture, shown, drawn)]);
                let level = (frame % 60) as f32 / 240.0;
                list.ClearRenderTargetView(*at, &[level, level, level, 1.0], None);
                list.ResourceBarrier(&[gpu::transition(picture, drawn, shown)]);
                list.Close()?;
                b.direct.queue.ExecuteCommandLists(&[Some(list.cast()?)]);
                let from = qpc();
                let shown = chain.Present(interval, flags);
                let after = qpc();
                shown.ok().context("presenting")?;
                occluded += u32::from(shown == OCCLUDED);
                *free = b.direct.signal()?;
                if frame >= 2 {
                    call.push(us(from, after));
                    if let Some(before) = before {
                        whole.push(us(before, after));
                    }
                }
                before = Some(after);
            }
            let note = if occluded > 0 {
                format!("{occluded} frames hidden behind other windows")
            } else {
                "no frame refused or hidden".to_owned()
            };
            b.row(
                Row::new("present", &format!("frame, {name}"), "frames", 1)
                    .cpu(&whole)
                    .checked(&note),
            );
            b.row(Row::new("present", &format!("Present call, {name}"), "calls", 1).cpu(&call));
        }
        b.direct.idle()?;
        drop(frames);
        ensure!(
            IsWindow(Some(window.0)).as_bool(),
            "the window went away while presenting"
        );
    }
    Ok(())
}
