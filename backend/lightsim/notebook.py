"""A starter Jupyter notebook for ``lightsim notebook``."""
from __future__ import annotations


def _cell(kind: str, text: str) -> dict:
    cell = {"cell_type": kind, "metadata": {}, "source": text.splitlines(keepends=True)}
    if kind == "code":
        cell.update(execution_count=None, outputs=[])
    return cell


def notebook(project: str, case: str) -> dict:
    """A notebook that runs ``case`` of ``project`` and plots two channels."""
    return {
        "nbformat": 4, "nbformat_minor": 5,
        "metadata": {"kernelspec": {"name": "python3", "display_name": "Python 3",
                                    "language": "python"}},
        "cells": [
            _cell("markdown", f"# LightSim: {case}\n\nRuns the case in this Python process "
                              f"(no app, no network) and plots the results. Needs pandas and "
                              f"matplotlib for the plot."),
            _cell("code", f"import lightsim as ls\n\nr = ls.run({project!r}, case={case!r})\n"
                          f"print(r.status)\nr.kpis"),
            _cell("code", "for k in r.summary:\n"
                          "    print(f\"{k.label}: {k.value} {k.unit}\""
                          " + (f\" (not valid: {k.not_valid})\" if k.not_valid else \"\"))"),
            _cell("code", "df = r.df\n"
                          "speed = [c for c in df.columns if c.endswith('· Speed')][:2]\n"
                          "df[speed].plot(xlabel='Time [s]', ylabel=df.attrs['units'][speed[0]]"
                          " if speed else '')"),
            _cell("code", "r.to_csv('results.csv')\nr.to_mat('results.mat')"),
        ],
    }
