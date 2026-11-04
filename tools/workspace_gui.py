#!/usr/bin/env python3
"""Small Tk desktop app for creating workspaces from repository templates."""

from __future__ import annotations

import re
import shutil
import tkinter as tk
from pathlib import Path
from tkinter import messagebox, ttk


ROOT = Path(__file__).resolve().parents[1]
TEMPLATES = ROOT / "templates"
WORKSPACES = ROOT / "workspaces"


class WorkspaceApp(tk.Tk):
    def __init__(self) -> None:
        super().__init__()
        self.title("OpenFlowSim Workspace Launcher")
        self.geometry("620x420")
        self.minsize(520, 360)

        self.template_paths = sorted(
            (path for path in TEMPLATES.iterdir() if path.is_dir()),
            key=lambda path: path.name.lower(),
        ) if TEMPLATES.is_dir() else []
        self.template_by_name = {path.name: path for path in self.template_paths}

        container = ttk.Frame(self, padding=20)
        container.pack(fill="both", expand=True)
        ttk.Label(container, text="Create a solver workspace", font=("TkDefaultFont", 16, "bold")).pack(anchor="w")
        ttk.Label(
            container,
            text="Choose a solver template and give this local workspace a name.",
        ).pack(anchor="w", pady=(4, 18))

        form = ttk.Frame(container)
        form.pack(fill="x")
        ttk.Label(form, text="Template").grid(row=0, column=0, sticky="w", pady=5)
        self.template_var = tk.StringVar(value=self.template_paths[0].name if self.template_paths else "")
        self.template_combo = ttk.Combobox(
            form,
            textvariable=self.template_var,
            values=list(self.template_by_name),
            state="readonly" if self.template_paths else "disabled",
        )
        self.template_combo.grid(row=0, column=1, sticky="ew", padx=(16, 0), pady=5)

        ttk.Label(form, text="Workspace name").grid(row=1, column=0, sticky="w", pady=5)
        self.name_var = tk.StringVar(value="channel")
        self.name_entry = ttk.Entry(form, textvariable=self.name_var)
        self.name_entry.grid(row=1, column=1, sticky="ew", padx=(16, 0), pady=5)
        form.columnconfigure(1, weight=1)

        self.info = tk.Text(container, height=8, wrap="word", state="disabled", borderwidth=1, relief="solid")
        self.info.pack(fill="both", expand=True, pady=(18, 12))
        self.template_combo.bind("<<ComboboxSelected>>", lambda _event: self.refresh_info())
        self.refresh_info()

        footer = ttk.Frame(container)
        footer.pack(fill="x")
        ttk.Label(footer, text=f"Workspaces are created in {WORKSPACES.relative_to(ROOT)}/ and ignored by Git.").pack(side="left")
        self.create_button = ttk.Button(footer, text="Create workspace", command=self.create_workspace, state="normal" if self.template_paths else "disabled")
        self.create_button.pack(side="right")

    def refresh_info(self) -> None:
        path = self.template_by_name.get(self.template_var.get())
        readme = path / "README.md" if path else None
        text = readme.read_text(encoding="utf-8") if readme and readme.is_file() else "No template description is available."
        self.info.configure(state="normal")
        self.info.delete("1.0", "end")
        self.info.insert("1.0", text)
        self.info.configure(state="disabled")

    def create_workspace(self) -> None:
        template = self.template_by_name.get(self.template_var.get())
        name = self.name_var.get().strip()
        if not template:
            messagebox.showerror("No template", "Choose an available template first.", parent=self)
            return
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", name):
            messagebox.showerror("Invalid name", "Use letters, numbers, underscores, or hyphens; start with a letter or number.", parent=self)
            return

        destination = WORKSPACES / name
        if destination.exists():
            messagebox.showerror("Workspace exists", f"{destination} already exists. Choose another name.", parent=self)
            return
        try:
            WORKSPACES.mkdir(parents=True, exist_ok=True)
            shutil.copytree(template, destination)
        except OSError as error:
            messagebox.showerror("Could not create workspace", str(error), parent=self)
            return
        messagebox.showinfo("Workspace created", f"Created {destination}\n\nYou can now edit its scenario and geometry files.", parent=self)


if __name__ == "__main__":
    WorkspaceApp().mainloop()
