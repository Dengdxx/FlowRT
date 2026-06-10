use crate::components::{Camera, Processor};
use crate::messages::FrameHandle;
use flowrt::{FrameDescriptorFields, FrameLeaseStatus, Latest, Output, Status};

#[derive(Debug, Default)]
struct CameraBoundary {
    generation: u64,
    boundary: Option<flowrt::BoundaryContext>,
}

impl Camera for CameraBoundary {
    fn on_start(&mut self, context: &mut flowrt::Context) -> Status {
        if let Some(boundary) = context.boundary() {
            boundary.mark_resource_ready("frames");
            boundary.mark_ready();
            boundary.report_healthy();
            self.boundary = Some(boundary.clone());
        }
        Status::ok()
    }

    fn on_tick(&mut self, frame: &mut Output<FrameHandle>) -> Status {
        self.generation += 1;
        let descriptor = FrameDescriptorFields {
            resource_id_hash: 0xF081,
            slot: 7,
            generation: self.generation,
            size_bytes: 640 * 480 * 3,
            timestamp_unix_ns: self.generation * 20_000_000,
            width: 640,
            height: 480,
            stride_bytes: 1_920,
            format_id: 1,
            encoding_id: 1,
            flags: 0,
        };
        if let Some(boundary) = &self.boundary {
            let _ = boundary.record_frame_descriptor_fields_event(
                "camera.frame",
                descriptor,
                FrameLeaseStatus::Acquired,
                false,
            );
        }
        frame.write(FrameHandle::from_frame_descriptor_fields(descriptor));
        Status::ok()
    }
}

#[derive(Debug, Default)]
struct FrameProcessor {
    observed: u64,
}

impl Processor for FrameProcessor {
    fn on_tick(&mut self, frame: Latest<'_, FrameHandle>) -> Status {
        let Some(frame) = frame.as_ref() else {
            return Status::Retry;
        };
        let descriptor = frame.frame_descriptor_fields();
        if descriptor.size_bytes == 0 || descriptor.width == 0 || descriptor.height == 0 {
            return Status::Error;
        }
        self.observed += 1;
        Status::ok()
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(CameraBoundary::default()),
        Box::new(FrameProcessor::default()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_app_runs_descriptor_path() {
        let backend = flowrt::iox2_backend();
        let status = build_app().run(&backend, Some(2));
        assert_eq!(status, Status::Ok);
    }
}
