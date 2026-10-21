//! GPU integration test for `CompositorConfig::frame_formats`.
//!
//! The gate: *with `frame_formats` set, every frame is allocated in one of the
//! listed formats.* An embedder that imports only some layouts lists them, and
//! the pool must not hand it any other.
//!
//! The test is `#[ignore]`d: it needs a render node that renders into linear
//! 8-bit layouts, and `vkcube`. Run it with:
//!
//! ```text
//! cargo test -p ferriscope --test frame_formats -- --ignored
//! ```

use std::{
	process::{Command, Stdio},
	time::{Duration, Instant},
};

use ferriscope::{Compositor, CompositorConfig, Format, Fourcc, Frame, FrameData, Modifier, RecvTimeoutError};
use smithay::backend::allocator::Buffer as _;

const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the test collects frames for.
const COLLECT: Duration = Duration::from_secs(3);

#[test]
#[ignore = "requires GPU and vkcube"]
fn frames_are_allocated_only_in_the_embedders_formats() {
	// Linear layouts only: Intel and AMD render 8-bit RGB linear, though never
	// as their first pick, so an unfiltered pool would use something else.
	// NVIDIA renders 8-bit RGB only in block-linear layouts.
	let linear: Vec<Format> = [Fourcc::Abgr8888, Fourcc::Xbgr8888, Fourcc::Argb8888, Fourcc::Xrgb8888]
		.into_iter()
		.map(|code| Format {
			code,
			modifier: Modifier::Linear,
		})
		.collect();
	let config = CompositorConfig {
		width: 320,
		height: 240,
		frame_formats: Some(linear.clone()),
		output: ferriscope::FrameOutput::Channel(ferriscope::ChannelOutput { pool_size: 2 }),
		..CompositorConfig::default()
	};
	let (compositor, handles) = Compositor::launch(config).expect("launch compositor");
	let ready = handles.ready.recv_timeout(READY_TIMEOUT).expect("ready handshake");
	let mut command = Command::new("vkcube");
	command
		.args(["--wsi", "wayland"])
		.env("WAYLAND_DISPLAY", &ready.wayland_display)
		.stdout(Stdio::null())
		.stderr(Stdio::null());
	if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
		command.env("XDG_RUNTIME_DIR", runtime_dir);
	}
	let mut vkcube = command.spawn().expect("spawn vkcube");

	let mut seen = Vec::new();
	let deadline = Instant::now() + COLLECT;
	while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
		match handles.frames.recv_timeout(remaining) {
			Ok(Frame {
				data: FrameData::DmaBuf(frame),
				..
			}) => seen.push(frame.dmabuf.format()),
			Ok(_) => {},
			Err(RecvTimeoutError::Timeout) => break,
			Err(RecvTimeoutError::Disconnected) => panic!("frame mailbox disconnected"),
		}
	}
	let _ = vkcube.kill();
	let _ = vkcube.wait();
	compositor.stop().expect("stop");

	assert!(!seen.is_empty(), "no DMA-BUF frame within {COLLECT:?}");
	for format in &seen {
		assert!(
			linear.contains(format),
			"a frame in {format:?}, which the embedder did not list"
		);
	}
}
