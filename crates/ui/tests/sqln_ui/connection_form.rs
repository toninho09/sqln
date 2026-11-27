// The connection form with a connection text.

use sqln_ui::form::{ConnectionForm, Field};

#[test]
fn the_form_text_refuses_an_option_of_the_command_line() {
    let mut form = ConnectionForm::new();
    form.use_uri = true;
    form.focus_on(Field::Name);
    form.on_paste("app");
    form.focus_on(Field::Uri);
    form.on_paste("postgres://u@h/d?pool=2");
    let error = form.definition().unwrap_err();
    assert_eq!(error.field, Field::Uri);
    assert_eq!(
        error.message,
        "pool is valid only on the command line. Use the form fields."
    );
}
