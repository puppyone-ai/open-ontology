import argparse
import json
from datetime import datetime
import pandas as pd


def load(path):
    with open(path) as f:
        return json.load(f)


def depth_by_day(frame):
    return frame.groupby("day").size()


def parse_args():
    p = argparse.ArgumentParser()
    p.add_argument("--since", default="7d")
    return p.parse_args()


if __name__ == "__main__":
    args = parse_args()
    print(depth_by_day(pd.DataFrame(load("jobs.json"))), datetime.now())
