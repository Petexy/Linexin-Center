"""Fixture: a module that fails while executing."""


class ImportErrorWidget:
    def __init__(self):
        self.widgetname = "Fixture Import Error"


raise ZeroDivisionError("fixture import failure")
