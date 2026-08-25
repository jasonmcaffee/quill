//! Letting the desktop show through the window on Windows.
//!
//! macOS needs nothing here: `ViewportBuilder::with_transparent(true)` is enough, the compositor takes
//! the surface's alpha and that is the end of it. Windows needs two separate things, and the window
//! stays solid if either one is missing. Both were found by measurement, with a probe that painted
//! half of a window at a quarter alpha over a coloured backdrop and read the pixels back.
//!
//! **The swapchain has to be able to carry alpha.** A DXGI swapchain made from a window handle offers
//! `CompositeAlphaMode::Opaque` and nothing else, so `egui-wgpu` has no transparent mode to ask for
//! and the window is composited solid however transparent egui paints it. A swapchain made from a
//! DirectComposition visual offers `PreMultiplied`, which `egui-wgpu` picks by itself once it is
//! offered. wgpu only builds that kind on its DX12 backend, and left to itself wgpu picked Vulkan on
//! the machine this was measured on — which is why setting `WGPU_DX12_PRESENTATION_SYSTEM=visual`
//! alone had never changed anything. So the backend is named as well as the swapchain kind.
//!
//! **The window's redirection surface has to be transparent.** Every ordinary window has one: a GDI
//! bitmap the desktop window manager composites the window from. Winit asks the manager to honour its
//! alpha, with `DwmEnableBlurBehindWindow` over an empty region, but registers the window class with
//! no background brush and never paints into it, so the surface keeps whatever undefined bytes it was
//! allocated with. Those bytes read as opaque white, which is exactly what the window showed: the
//! theme faded towards white rather than towards the desktop. GDI writes zero into the alpha byte of
//! every pixel it touches, so one black fill over the whole client area is what makes the surface
//! disappear. This is the same fault GLFW has, and the fix is the one its own bug report proposes.
//!
//! `WS_EX_NOREDIRECTIONBITMAP` removes the surface instead of clearing it, and was measured to work
//! too. It is not used: eframe gives no way to pass a winit window attribute through, and the Win32
//! reference implementations advise against it because it breaks any presentation path that blits
//! into that surface.

use eframe::wgpu::{Backends, Dx12SwapchainKind};
use eframe::wgpu::rwh::{HasWindowHandle, RawWindowHandle};

/// Ask wgpu for the one swapchain on Windows that can be translucent.
///
/// Both choices still respect wgpu's own environment variables, so `WGPU_BACKEND` and
/// `WGPU_DX12_PRESENTATION_SYSTEM` remain a way out on a machine where DX12 is not the right answer.
/// The window is then opaque, which is what it was before this existed.
pub fn with_direct_composition(mut options: eframe::NativeOptions) -> eframe::NativeOptions {
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = Backends::from_env().unwrap_or(Backends::DX12);
        setup.instance_descriptor.backend_options.dx12.presentation_system =
            Dx12SwapchainKind::DxgiFromVisual.with_env();
    }
    options
}

/// Keep the window's redirection surface transparent, once a frame.
///
/// It is filled every frame rather than only when it looks as though Windows has given the window a
/// new one. Filling once is not enough and there is no event that says when it is: a fill while the
/// window is still hidden is thrown away, and eframe deliberately keeps the window hidden until it
/// has painted its first frame, so the obvious place to do it once is exactly the place that does
/// not work. Guessing at the rest — a resize, a move to a screen at another scale, a restore from
/// the taskbar — would be a list to keep up to date rather than a rule.
///
/// It costs nothing worth saving. The fill was measured at 0.06 ms for a 1100 by 720 window and
/// 0.04 ms for one filling a 4K screen, because it is a write into a surface the compositor owns
/// rather than anything the graphics card has to draw.
pub fn keep_transparent(window: &impl HasWindowHandle) {
    let Some(hwnd) = window_handle(window) else { return };
    let Some((width, height)) = client_size(hwnd) else { return };
    fill(hwnd, width, height);
}

/// The Win32 window handle behind an eframe window, as an integer so that nothing holds a pointer.
///
/// Absent when there is no window at all, which is the case in every screenshot test: those render
/// offscreen and have no desktop to show through.
fn window_handle(window: &impl HasWindowHandle) -> Option<isize> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

/// The window's client area, in pixels.
fn client_size(hwnd: isize) -> Option<(i32, i32)> {
    let mut rect = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let read = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd as _, &mut rect) };
    if read == 0 {
        return None;
    }
    Some((rect.right - rect.left, rect.bottom - rect.top))
}

/// Fill the whole client area with black through GDI, which sets every pixel's alpha to zero.
fn fill(hwnd: isize, width: i32, height: i32) {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, PatBlt, ReleaseDC, BLACKNESS};
    unsafe {
        let device_context = GetDC(hwnd as _);
        if device_context.is_null() {
            return;
        }
        PatBlt(device_context, 0, 0, width, height, BLACKNESS);
        ReleaseDC(hwnd as _, device_context);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether the window can be translucent at all is decided here, before a window exists, so it is
    /// worth a test: the two settings together are the difference between a window that blends and one
    /// that is solid, and neither of them is visible in anything the tests can render.
    #[test]
    fn the_swapchain_is_asked_for_on_the_one_backend_that_can_carry_alpha() {
        let options = with_direct_composition(eframe::NativeOptions::default());
        let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &options.wgpu_options.wgpu_setup else {
            panic!("eframe is expected to be creating its own wgpu instance");
        };
        assert_eq!(
            setup.instance_descriptor.backends,
            Backends::DX12,
            "Vulkan offers no transparent composite mode on Windows, so the backend has to be named"
        );
        assert_eq!(
            setup.instance_descriptor.backend_options.dx12.presentation_system,
            Dx12SwapchainKind::DxgiFromVisual,
            "a swapchain made from the window handle can only be opaque"
        );
    }

    /// Nothing happens when there is no window, which is what every screenshot test has.
    #[test]
    fn a_handle_with_no_window_behind_it_is_left_alone() {
        struct Nothing;
        impl HasWindowHandle for Nothing {
            fn window_handle(
                &self,
            ) -> Result<eframe::wgpu::rwh::WindowHandle<'_>, eframe::wgpu::rwh::HandleError> {
                Err(eframe::wgpu::rwh::HandleError::NotSupported)
            }
        }
        keep_transparent(&Nothing);
    }
}

