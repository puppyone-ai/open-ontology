"""Export the queue to CSV."""
import csv
import sqlite3
import click
import requests
from rich.console import Console

console = Console()


class Exporter:
    def __init__(self, path):
        self.path = path

    def rows(self):
        db = sqlite3.connect(self.path)
        return db.execute("select id, kind from jobs").fetchall()

    def write(self, out):
        def helper(row):
            return row

        with open(out, "w") as f:
            csv.writer(f).writerows(helper(r) for r in self.rows())


@click.command()
def main():
    """def fake_function(): pass"""
    Exporter("jobs.db").write("jobs.csv")
    requests.post("https://example.invalid/hook")
    console.print("done")


def summarize(rows):
    return len(rows)
