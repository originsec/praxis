use super::M365CopilotAgent;
use crate::utils;

impl M365CopilotAgent {
    pub(super) async fn do_fingerprint_impl(&self) -> bool {
        let process_name = "M365Copilot.exe";

        //
        // Check for resident process.
        //

        if let Some(path) = utils::get_running_process_path(process_name) {
            let _ = self.process_path.set(path);
            return true;
        }

        //
        // Find in Windows package install location.
        //

        let package_path =
            utils::get_package_install_path("Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe")
                .unwrap_or_default();
        if utils::find_file_in_path(process_name, &package_path) {
            let _ = self
                .process_path
                .set(format!("{}\\{}", package_path, process_name));
            return true;
        }
        
        //
        // (Note: There are other/better/more straight-forward ways to 
        // fingerprint but seems sufficient for now.)
        //

        false
    }
}
