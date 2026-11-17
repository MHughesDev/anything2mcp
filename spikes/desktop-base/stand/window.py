#!/usr/bin/python3.12
# Throwaway test window for roadmap package 8.1. A small GTK 3 window with
# standard controls. "Save" writes what the form holds to the file named by the
# first argument, which is the program's store: the spike asks it, never its
# own read path, whether a write happened. Run with the system Python, which
# has the GObject bindings.
import json
import sys

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

STORE = sys.argv[1]
GLib.set_prgname("example-stand")
GLib.set_application_name("example-stand")

win = Gtk.Window(title="Example line item L-101")
grid = Gtk.Grid(column_spacing=8, row_spacing=8, margin=12)
win.add(grid)


def labelled(row, text, widget):
    label = Gtk.Label(label=text, xalign=0)
    label.set_mnemonic_widget(widget)  # the labelled-by relation gives the field its name
    grid.attach(label, 0, row, 1, 1)
    grid.attach(widget, 1, row, 1, 1)
    return widget


description = labelled(0, "Description", Gtk.Entry(text="Hinge"))
quantity = labelled(1, "Quantity", Gtk.Entry(text="4"))
pin = labelled(2, "PIN", Gtk.Entry(text="example-pin-1234", visibility=False))
taxable = Gtk.CheckButton(label="Taxable")
grid.attach(taxable, 1, 3, 1, 1)
status = Gtk.Label(label="Not saved", xalign=0)
grid.attach(status, 1, 5, 1, 1)
save = Gtk.Button(label="Save")
grid.attach(save, 1, 4, 1, 1)
writes = 0


def on_save(_button):
    global writes
    writes += 1
    record = {
        "writes": writes,
        "description": description.get_text(),
        "quantity": quantity.get_text(),
        "taxable": taxable.get_active(),
    }
    with open(STORE, "w", encoding="utf-8") as f:
        json.dump(record, f)
    status.set_text("Saved")


save.connect("clicked", on_save)
win.connect("destroy", Gtk.main_quit)
win.show_all()
print("ready", flush=True)
Gtk.main()
