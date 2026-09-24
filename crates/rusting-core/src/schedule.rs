/// The ordered stages executed by an application's `update` method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScheduleStage {
    Startup,
    FixedUpdate,
    Update,
    PostUpdate,
    RenderExtract,
}

/// Summary of work performed during a single application update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameReport {
    pub fixed_steps: u32,
    pub exit_requested: bool,
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    use std::hash::Hash;

    use super::{FrameReport, ScheduleStage};

    #[test]
    fn frame_report_default_has_no_work_or_exit_request() {
        assert_eq!(
            FrameReport::default(),
            FrameReport {
                fixed_steps: 0,
                exit_requested: false,
            }
        );
    }

    #[test]
    fn schedule_stage_variants_are_distinct_and_debuggable() {
        let stages = [
            ScheduleStage::Startup,
            ScheduleStage::FixedUpdate,
            ScheduleStage::Update,
            ScheduleStage::PostUpdate,
            ScheduleStage::RenderExtract,
        ];

        for (index, stage) in stages.iter().enumerate() {
            assert!(!stages[..index].contains(stage));
        }
        assert_eq!(
            format!("{:?}", ScheduleStage::RenderExtract),
            "RenderExtract"
        );
    }

    #[test]
    fn schedule_types_preserve_their_public_trait_shape() {
        fn assert_stage_traits<T: Clone + Copy + Debug + Eq + Hash>() {}
        fn assert_report_traits<T: Clone + Copy + Debug + Default + Eq>() {}

        assert_stage_traits::<ScheduleStage>();
        assert_report_traits::<FrameReport>();
    }

    #[test]
    fn frame_report_fields_are_publicly_constructible_and_readable() {
        let report = FrameReport {
            fixed_steps: 3,
            exit_requested: true,
        };

        assert_eq!(report.fixed_steps, 3);
        assert!(report.exit_requested);
    }
}
