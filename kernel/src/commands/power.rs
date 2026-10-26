use uefi::runtime::ResetType;
use uefi::Status;

pub(super) fn shutdown(output_y: u32) -> ! {
    super::draw_line(output_y, "Shutting down...", 255, 255, 255);
    uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None)
}

pub(super) fn restart(output_y: u32) -> ! {
    super::draw_line(output_y, "Restarting...", 255, 255, 255);
    uefi::runtime::reset(ResetType::COLD, Status::SUCCESS, None)
}
