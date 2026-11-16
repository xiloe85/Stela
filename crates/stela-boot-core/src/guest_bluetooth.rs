//! The boot log's one line about the guest's Bluetooth. macOS gets no
//! Bluetooth controller in either case: with a host controller, because
//! bluetoothd never configures the guest's USB one and Setup waits for it;
//! without one, because there is nothing to lend and no built-in controller.
//! Either way bluetoothd's chip boot fails, it exits and launchd starts it
//! again about every 10 s (its KeepAlive restarts a failed exit).

/// The line for a boot that gives macOS host input, by whether the host
/// lends a Bluetooth controller; `None` for a boot without host input.
pub fn unsupported_line(host_controller: bool, input: bool) -> Option<&'static str> {
    if !input {
        return None;
    }
    Some(if host_controller {
        "BLUETOOTH UNSUPPORTED the guest gets no controller: macOS does not \
         configure the guest's USB Bluetooth controller and its Setup waits for it\n"
    } else {
        "BLUETOOTH UNSUPPORTED the guest gets no controller: this computer has none to \
         lend and Stela has no built-in one, so macOS's Bluetooth daemon restarts about \
         every 10 s\n"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boot_without_a_host_controller_still_names_the_missing_guest_controller() {
        let line = unsupported_line(false, true).expect("reported");
        assert!(line.starts_with("BLUETOOTH UNSUPPORTED the guest gets no controller: "));
        assert!(line.contains("Bluetooth daemon restarts"));
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
    }

    #[test]
    fn a_host_controller_keeps_its_reason_and_a_boot_without_input_says_nothing() {
        let line = unsupported_line(true, true).expect("reported");
        assert!(line.contains("does not configure the guest's USB Bluetooth controller"));
        assert_ne!(Some(line), unsupported_line(false, true));
        assert_eq!(unsupported_line(true, false), None);
        assert_eq!(unsupported_line(false, false), None);
    }
}
