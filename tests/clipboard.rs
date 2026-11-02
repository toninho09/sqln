// The alternative of the system clipboard: the terminal copies the text.

use std::io::{self, Write};

use sqln::clipboard::{Clipboard, CopyOutcome, SystemClipboard, base64, write_osc52};

/// A terminal that closed. Each write fails.
struct ClosedTerminal;

impl Write for ClosedTerminal {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_terminal_copy_carries_the_text_in_base64() {
    let mut output = Vec::new();
    assert_eq!(
        write_osc52(&mut output, "hello"),
        CopyOutcome::SentToTerminal
    );
    assert_eq!(output, b"\x1b]52;c;aGVsbG8=\x07");
}

#[test]
fn a_failed_write_reports_a_failed_copy() {
    assert_eq!(
        write_osc52(&mut ClosedTerminal, "hello"),
        CopyOutcome::Failed
    );
}

#[test]
fn base64_pads_each_length() {
    for (text, encoded) in [("", ""), ("a", "YQ=="), ("ab", "YWI="), ("abc", "YWJj")] {
        assert_eq!(base64(text.as_bytes()), encoded);
    }
}

#[test]
fn a_clipboard_without_a_system_pastes_nothing() {
    assert_eq!(SystemClipboard::default().paste(), None);
}
