//! Throwaway test window: the same form as `stand/window.py`, in egui.
//! "Save" writes the form to the store file named by the first argument.
use eframe::egui;

struct Form { store: String, description: String, quantity: String, taxable: bool, writes: u32, status: String }

impl eframe::App for Form {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            let l = ui.label("Description");
            ui.text_edit_singleline(&mut self.description).labelled_by(l.id);
            let l = ui.label("Quantity");
            ui.text_edit_singleline(&mut self.quantity).labelled_by(l.id);
            ui.checkbox(&mut self.taxable, "Taxable");
            if ui.button("Save").clicked() {
                self.writes += 1;
                let record = format!(
                    "{{\"writes\": {}, \"description\": {:?}, \"quantity\": {:?}, \"taxable\": {}}}",
                    self.writes, self.description, self.quantity, self.taxable
                );
                let _ = std::fs::write(&self.store, record);
                self.status = "Saved".into();
            }
            ui.label(&self.status);
        });
    }
}

fn main() -> eframe::Result {
    let store = std::env::args().nth(1).unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Example line item L-101").with_app_id("example-stand"),
        ..Default::default()
    };
    println!("ready");
    eframe::run_native("example-stand", options, Box::new(|_| Ok(Box::new(Form {
        store, description: "Hinge".into(), quantity: "4".into(), taxable: false, writes: 0, status: "Not saved".into(),
    }))))
}
