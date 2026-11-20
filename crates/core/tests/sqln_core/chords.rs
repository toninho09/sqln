use sqln_core::input::Chord;

#[test]
fn constant_chords_match_their_configuration_text() {
    assert_eq!(Chord::ESC, Chord::parse("esc").unwrap());
    assert_eq!(Chord::ctrl('w'), Chord::parse("ctrl+w").unwrap());
    assert_eq!(Chord::ctrl('y'), Chord::parse("ctrl+y").unwrap());
}
