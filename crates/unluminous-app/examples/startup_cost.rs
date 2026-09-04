//! What starting Unluminous costs, piece by piece, on this machine.
//!
//! `UNLUMINOUS_FRAME_TRACE` says when the program had got to each step of `main`, which is what
//! `task-1805` used to find that a third of startup was a stale instance file being dialled. It
//! could say nothing about the one step that is not Unluminous's own code — creating the window and
//! the graphics device, which the marks showed at about 400 ms of an 811 ms startup and no more.
//!
//! This is that step taken apart. It builds a graphics instance, asks for an adapter, asks for a
//! device and compiles egui's shader exactly as `eframe` does, and prints what each cost. The rest
//! of startup is here too, at the sizes it really is on this machine's own disk: the font database,
//! the plugins and the project walk.
//!
//! ```text
//! cargo run --release -p unluminous-app --example startup_cost -- [folder] [--no-graphics]
//! ```
//!
//! **`--no-graphics` is how the memory question is answered.** Run it both ways, watching the
//! process from outside, and the difference is what the graphics driver holds. `task-1805` measured
//! **139 MB of working set and 318 MB of private bytes** for a process that had done nothing but ask
//! for a DX12 device, against a whole Unluminous window's 220 MB and 435 MB — so most of what an
//! editor drawn on the graphics card holds is the driver's rather than the editor's, and there is no
//! amount of care inside Unluminous that would get it back.
//!
//! It is a **diagnostic and not a test**, for the reason `frame_cost` gives: every number here is a
//! different number on another machine, and a threshold would fail on the next one.

use std::time::Instant;

fn ms(from: Instant) -> f64 {
    from.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    println!("Starting Unluminous, piece by piece. Times are milliseconds on this machine.");
    println!();

    if !std::env::args().any(|argument| argument == "--no-graphics") {
        measure_the_graphics();
    }

    let start = Instant::now();
    let renderer = unluminous_app::services::text_renderer::TextRenderer::new();
    println!(
        "  font database                 {:.1}   ({} families offered)",
        ms(start),
        renderer.families().len()
    );

    let start = Instant::now();
    let (plugins, _) = unluminous_app::services::plugins::Plugins::load(None);
    println!("  plugins                       {:.1}   ({} read)", ms(start), plugins.all().len());

    let folder = std::env::args()
        .skip(1)
        .find(|argument| !argument.starts_with("--"))
        .unwrap_or_else(|| ".".to_owned());
    let start = Instant::now();
    let tree = unluminous_app::services::file_tree::FileTree::new(&folder);
    println!(
        "  walk the project              {:.1}   ({} files, {} can be opened, in {folder})",
        ms(start),
        tree.file_count(),
        tree.openable_count()
    );
}

/// The graphics device, asked for exactly as `eframe` asks for it.
///
/// This is the step `main`'s own marks cannot see inside, and on this machine it is more than half
/// of the time to a window and most of the memory behind one.
fn measure_the_graphics() {
    // Asked for exactly as `services::windows_transparency` asks eframe to ask for it, so the number
    // means something about the real window rather than about a default nobody uses.
    let options = eframe::NativeOptions::default();
    #[cfg(windows)]
    let options = unluminous_app::services::windows_transparency::with_direct_composition(options);
    let mut descriptor = eframe::wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &options.wgpu_options.wgpu_setup {
        descriptor.backends = setup.instance_descriptor.backends;
        descriptor.backend_options.dx12 = setup.instance_descriptor.backend_options.dx12.clone();
    }
    println!("  backends asked for            {:?}", descriptor.backends);

    let start = Instant::now();
    let instance = eframe::wgpu::Instance::new(descriptor);
    println!("  graphics instance             {:.1}", ms(start));

    // In the order eframe asks, because asking in any other order measures a warm driver: listing
    // the adapters is the expensive half of finding one, so doing that first reports two
    // milliseconds for a step that really costs the best part of a second.
    let start = Instant::now();
    let adapter =
        pollster_block(instance.request_adapter(&eframe::wgpu::RequestAdapterOptions::default()));
    println!("  request adapter               {:.1}", ms(start));
    let Ok(adapter) = adapter else {
        println!("  no adapter on this machine, so there is nothing more to measure here");
        return;
    };
    println!("      chosen: {}", adapter.get_info().name);

    let start = Instant::now();
    let device = pollster_block(adapter.request_device(&eframe::wgpu::DeviceDescriptor {
        label: Some("unluminous startup probe"),
        ..Default::default()
    }));
    println!("  request device                {:.1}", ms(start));
    let Ok((device, _queue)) = device else {
        println!("  the adapter would not give a device");
        return;
    };

    // The one shader egui draws everything with. On the DX12 backend this goes through a shader
    // compiler that has to be loaded and run, so it is worth having its own number.
    let start = Instant::now();
    let _renderer = eframe::egui_wgpu::Renderer::new(
        &device,
        eframe::wgpu::TextureFormat::Bgra8UnormSrgb,
        eframe::egui_wgpu::RendererOptions::default(),
    );
    println!("  compile egui's shader         {:.1}", ms(start));
    println!();
}

/// Run a future to completion on this thread.
///
/// wgpu's requests are futures that are already resolved on the native backends, so this needs no
/// runtime — which is the whole point, because Unluminous has none and this example must not be the
/// thing that adds one. It is the same handful of lines `pollster` is.
fn pollster_block<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    fn nothing(_: *const ()) {}
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, nothing, nothing, nothing);
    // Safety: the waker does nothing at all, which is valid for a future that never yields.
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
