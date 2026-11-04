#!/usr/bin/env python3
"""OpenFlowSim STEP inspection and boundary tagging desktop prototype."""

from __future__ import annotations

import sys
import hashlib
import json
from math import sqrt
from pathlib import Path

VENDOR_ROOT = Path(__file__).resolve().parent / "step_viewer" / "vendor"
sys.path.insert(0, str(VENDOR_ROOT))
REPO_ROOT = Path(__file__).resolve().parents[1]

from PySide6.QtCore import Qt, QSettings, QSize
from PySide6.QtGui import QAction, QKeySequence, QPixmap
from PySide6.QtWidgets import (
    QApplication,
    QComboBox,
    QFileDialog,
    QFormLayout,
    QFrame,
    QHBoxLayout,
    QInputDialog,
    QLabel,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QDoubleSpinBox,
    QScrollArea,
    QStatusBar,
    QVBoxLayout,
    QWidget,
)

from step_explorer.geometry.builder import GeometryBuilder
from step_explorer.step.parser import StepDocument
from step_explorer.step.values import StepAggregate, StepReference
from step_explorer.visualization.vtk_view import VtkView
from step_explorer.vtk_lite import vtk


STYLE = """
QMainWindow, QWidget { background: #171b22; color: #e7ecf3; font-size: 14px; }
QToolBar { background: #202630; border: 0; spacing: 8px; padding: 8px 12px; }
QToolButton#openStepButton { background: #3b82f6; border: 1px solid #3b82f6;
  border-radius: 6px; padding: 7px 12px; color: white; font-weight: 600; }
QToolButton#openStepButton:hover { background: #5594fa; border-color: #5594fa; }
QToolButton#openStepButton:pressed { background: #2563eb; }
QPushButton { background: #2a3442; border: 1px solid #3a4656; border-radius: 7px;
  padding: 8px 14px; color: #e7ecf3; }
QPushButton:hover { background: #354357; border-color: #7186a2; }
QPushButton:default { background: #3b82f6; border-color: #3b82f6; color: white; }
QComboBox, QDoubleSpinBox { background: #202630; border: 1px solid #3a4656;
  border-radius: 5px; padding: 6px; min-height: 22px; }
QLabel#muted { color: #98a5b5; }
QLabel#selectionModeLegend { color: #dce6f2; padding: 0 10px; }
QStatusBar { background: #202630; color: #98a5b5; }
"""


class OpenFlowWindow(QMainWindow):
    def __init__(self, workspace_dir: Path | None = None) -> None:
        super().__init__()
        self.workspace_dir = workspace_dir
        self.setWindowTitle((f"OpenFlowSim · {workspace_dir.name}" if workspace_dir else
                             "OpenFlowSim · Geometry Setup") + "[*]")
        self.setWindowModified(False)
        self.dirty = False
        self.resize(1440, 900)
        self.setMinimumSize(980, 640)
        self.step_path: Path | None = None
        self.snapshot = None
        self.document = None
        self.geometry_builder = None
        self.selected_face: int | None = None
        self.selected_entity_id: int | None = None
        self.face_roles: dict[int, str] = {}
        self.opening_points = None
        self.cap_actor = None

        toolbar = self.addToolBar("Project")
        toolbar.setMovable(False)
        open_action = QAction("Open STEP…", self)
        open_action.setShortcut(QKeySequence.StandardKey.Open)
        open_action.triggered.connect(self.open_step)
        toolbar.addAction(open_action)
        open_button = toolbar.widgetForAction(open_action)
        open_button.setObjectName("openStepButton")
        toolbar.addSeparator()
        toolbar.addWidget(QLabel("Select a face, then assign a boundary role"))
        toolbar.addSeparator()
        save_action = QAction("Save Workspace", self)
        save_action.setShortcut(QKeySequence.StandardKey.Save)
        save_action.triggered.connect(self.save_workspace)
        toolbar.addAction(save_action)

        self.viewport = VtkView(
            self,
            navigation_preset="Blender",
            invert_mouse_vertical=True,
            autozoom=False,
            background_color="#263342",
            show_xyz_origin=True,
            show_xy_grid=True,
        )
        self.viewport.face_picked.connect(self.select_face)
        self.viewport.edge_picked.connect(self.select_edge)
        self.viewport.opening_edge_picked.connect(self.select_opening_edge)
        self.viewport.vertex_picked.connect(self.select_vertex)
        self.selection_mode = "face"

        panel = QWidget()
        panel.setFixedWidth(310)
        panel_layout = QVBoxLayout(panel)
        panel_layout.setContentsMargins(18, 18, 18, 18)
        panel_layout.setSpacing(12)
        title = QLabel("Flow domain")
        title.setStyleSheet("font-size: 20px; font-weight: 650;")
        panel_layout.addWidget(title)
        panel_layout.addWidget(QLabel("Auto-fit bounds from the imported STEP scene."))

        self.bounds: dict[str, QDoubleSpinBox] = {}
        form = QFormLayout()
        form.setVerticalSpacing(9)
        for axis in "xyz":
            low, high = self._spin(), self._spin()
            self.bounds[f"{axis}min"] = low
            self.bounds[f"{axis}max"] = high
            low.valueChanged.connect(self.mark_dirty)
            high.valueChanged.connect(self.mark_dirty)
            row = QHBoxLayout()
            row.addWidget(low)
            row.addWidget(QLabel("to"))
            row.addWidget(high)
            form.addRow(axis.upper(), row)
        panel_layout.addLayout(form)
        panel_layout.addSpacing(10)

        panel_layout.addWidget(QLabel("Selected face"))
        self.face_label = QLabel("None — click a surface in the viewport")
        self.face_label.setObjectName("muted")
        self.face_label.setWordWrap(True)
        panel_layout.addWidget(self.face_label)
        self.role = QComboBox()
        self.role.addItems(["inlet", "outlet", "wall", "symmetry", "other"])
        panel_layout.addWidget(self.role)
        assign = QPushButton("Assign boundary  ·  Enter")
        assign.setDefault(True)
        assign.clicked.connect(self.assign_role)
        panel_layout.addWidget(assign)
        self.assign_action = QAction(self)
        self.assign_action.setShortcut(QKeySequence(Qt.Key.Key_Return))
        self.assign_action.triggered.connect(self.assign_role)
        self.addAction(self.assign_action)
        self.assign_action2 = QAction(self)
        self.assign_action2.setShortcut(QKeySequence("W"))
        self.assign_action2.triggered.connect(lambda: self.assign_quick("wall"))
        self.addAction(self.assign_action2)
        self.assign_action3 = QAction(self)
        self.assign_action3.setShortcut(QKeySequence("I"))
        self.assign_action3.triggered.connect(lambda: self.assign_quick("inlet"))
        self.addAction(self.assign_action3)
        self.assign_action4 = QAction(self)
        self.assign_action4.setShortcut(QKeySequence("O"))
        self.assign_action4.triggered.connect(lambda: self.assign_quick("outlet"))
        self.addAction(self.assign_action4)

        self.roles_label = QLabel("No boundary faces assigned")
        self.roles_label.setObjectName("muted")
        self.roles_label.setWordWrap(True)
        panel_layout.addWidget(self.roles_label)
        panel_layout.addStretch(1)
        hint = QLabel("Orbit: middle-drag · Pan: Shift + middle-drag · Zoom: wheel · F: focus selection")
        hint.setObjectName("muted")
        hint.setWordWrap(True)
        panel_layout.addWidget(hint)

        content = QWidget()
        layout = QHBoxLayout(content)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)
        layout.addWidget(self.viewport, 1)
        layout.addWidget(panel)
        self.setCentralWidget(content)
        self.setStatusBar(QStatusBar())
        self.statusBar().showMessage("Open a STEP file to begin")
        self.mode_indicator = QLabel()
        self.mode_indicator.setObjectName("selectionModeLegend")
        self.statusBar().addPermanentWidget(self.mode_indicator)
        self._mode_actions = []
        for key, mode in (("1", "vertex"), ("2", "edge"), ("3", "face"), ("4", "solid")):
            action = QAction(self)
            action.setShortcut(QKeySequence(key))
            action.setShortcutContext(Qt.ShortcutContext.WindowShortcut)
            action.triggered.connect(lambda _checked=False, selected=mode: self.set_selection_mode(selected))
            self.addAction(action)
            self._mode_actions.append(action)
        focus_action = QAction(self)
        focus_action.setShortcut(QKeySequence("F"))
        focus_action.setShortcutContext(Qt.ShortcutContext.WindowShortcut)
        focus_action.triggered.connect(self.focus_selection)
        self.addAction(focus_action)
        self.set_selection_mode("face")
        self.setStyleSheet(STYLE)

    @staticmethod
    def _spin() -> QDoubleSpinBox:
        spin = QDoubleSpinBox()
        spin.setRange(-1e9, 1e9)
        spin.setDecimals(4)
        spin.setSingleStep(0.1)
        return spin

    def open_step(self) -> None:
        filename, _ = QFileDialog.getOpenFileName(
            self, "Import STEP scene", "", "STEP files (*.step *.stp *.STEP *.STP)"
        )
        if not filename:
            return
        self.import_step_path(Path(filename))

    def set_selection_mode(self, mode: str) -> None:
        if mode not in {"vertex", "edge", "face", "solid"}:
            return
        self.selection_mode = mode
        self.viewport.selection_mode = mode
        labels = (("1", "Vertex"), ("2", "Edge"), ("3", "Face"), ("4", "Solid"))
        parts = []
        for key, label in labels:
            item = f"{key} {label}"
            if label.lower() == mode:
                parts.append(f'<span style="color:#78b7ff;font-weight:700">{item}</span>')
            else:
                parts.append(f'<span style="color:#98a5b5">{item}</span>')
        self.mode_indicator.setText("Select: &nbsp;" + " &nbsp;·&nbsp; ".join(parts))
        self.statusBar().showMessage(f"{mode.title()} selection mode")

    def select_vertex(self, entity_id: int) -> None:
        self.selected_face = None
        self.selected_entity_id = entity_id
        self.opening_points = None
        self.face_label.setText(f"STEP vertex #{entity_id}")
        if self.snapshot is not None:
            self.viewport.show_snapshot(self.snapshot, selected_id=entity_id)
        self.statusBar().showMessage("Vertex selected")

    def select_edge(self, entity_id: int) -> None:
        self.selected_face = None
        self.selected_entity_id = entity_id
        self.opening_points = None
        self._remove_cap()
        self.face_label.setText(f"STEP edge #{entity_id}")
        if self.snapshot is not None:
            self.viewport.show_snapshot(self.snapshot, selected_id=entity_id)
        self.statusBar().showMessage("Edge selected · right-click an aperture edge to define an opening cap")

    def import_step_path(self, path: Path) -> None:
        self.statusBar().showMessage(f"Reading and tessellating {path.name}…")
        QApplication.setOverrideCursor(Qt.CursorShape.WaitCursor)
        try:
            document = StepDocument.from_file(path)
            builder = GeometryBuilder(document)
            snapshot = builder.build()
            if not snapshot.faces:
                raise ValueError("No renderable faces were found in this STEP file.")
            self.step_path = path
            self.document = document
            self.geometry_builder = builder
            self.snapshot = snapshot
            self.face_roles.clear()
            self._remove_cap()
            self.viewport.show_snapshot(snapshot)
            self._fit_domain(snapshot)
            self.statusBar().showMessage(f"{path.name} · {len(snapshot.faces)} faces")
            self._refresh_roles()
            self.mark_dirty()
        except Exception as error:
            QMessageBox.critical(self, "STEP import failed", str(error))
            self.statusBar().showMessage("Import failed")
        finally:
            QApplication.restoreOverrideCursor()

    def load_workspace(self, project_path: Path) -> None:
        payload = json.loads(project_path.read_text(encoding="utf-8"))
        source = Path(payload["step_path"])
        if not source.is_file():
            raise FileNotFoundError(f"STEP source is missing: {source}")
        current_hash = hashlib.sha256(source.read_bytes()).hexdigest()
        hash_matches = current_hash == payload.get("step_sha256")
        self.import_step_path(source)
        if not hash_matches:
            QMessageBox.warning(
                self, "STEP geometry changed",
                "The STEP file no longer matches this workspace. Saved face assignments were discarded; please select them again.",
            )
            return
        self.face_roles = {int(key): value for key, value in payload.get("face_roles", {}).items()}
        for key, value in payload.get("domain", {}).items():
            if key in self.bounds:
                self.bounds[key].setValue(float(value))
        self._refresh_roles()
        self.dirty = False
        self.setWindowModified(False)

    def save_workspace(self) -> bool:
        if self.step_path is None:
            QMessageBox.information(self, "No STEP scene", "Import a STEP scene before saving this workspace.")
            return False
        if self.workspace_dir is None:
            folder = QFileDialog.getExistingDirectory(
                self, "Choose workspace folder", str(REPO_ROOT / "workspaces")
            )
            if not folder:
                return False
            self.workspace_dir = Path(folder)
        self.workspace_dir.mkdir(parents=True, exist_ok=True)
        digest = hashlib.sha256(self.step_path.read_bytes()).hexdigest()
        payload = {
            "format": "OpenFlowSim workspace",
            "format_version": 1,
            "writer_version": "0.1.0",
            "step_path": str(self.step_path.resolve()),
            "step_sha256": digest,
            "domain": {key: self.bounds[key].value() for key in self.bounds},
            "face_roles": {str(key): value for key, value in self.face_roles.items()},
        }
        project_path = self.workspace_dir / "workspace.ofs"
        try:
            project_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        except OSError as error:
            QMessageBox.critical(self, "Could not save workspace", str(error))
            return False
        image = self.viewport.capture_frame()
        if image is not None:
            preview = image.scaled(
                QSize(200, 100), Qt.AspectRatioMode.IgnoreAspectRatio,
                Qt.TransformationMode.SmoothTransformation,
            )
            preview.save(str(self.workspace_dir / "preview.png"), "PNG")
        WelcomeWindow.record_recent(self.workspace_dir)
        self.dirty = False
        self.setWindowModified(False)
        self.statusBar().showMessage(f"Saved workspace to {project_path}")
        QMessageBox.information(self, "Workspace saved", f"Saved {project_path}")
        return True

    def mark_dirty(self, *_args) -> None:
        if not self.dirty:
            self.dirty = True
            self.setWindowModified(True)

    def _fit_domain(self, snapshot) -> None:
        points = [point for face in snapshot.faces for point in face.points]
        for i, axis in enumerate("xyz"):
            lo = min(getattr(point, axis) for point in points)
            hi = max(getattr(point, axis) for point in points)
            padding = max((hi - lo) * 0.1, 0.01)
            self.bounds[f"{axis}min"].setValue(lo - padding)
            self.bounds[f"{axis}max"].setValue(hi + padding)

    def select_face(self, entity_id: int) -> None:
        self.selected_face = entity_id
        self.selected_entity_id = entity_id
        self.opening_points = None
        assigned = self.face_roles.get(entity_id)
        if self.selection_mode == "solid":
            solid_id = self._solid_containing_face(entity_id)
            self.face_label.setText(
                f"STEP solid #{solid_id} · face #{entity_id}" if solid_id is not None
                else f"No containing STEP solid · face #{entity_id}"
            )
        else:
            self.face_label.setText(f"STEP face #{entity_id}" + (f" · {assigned}" if assigned else ""))
        if self.snapshot is not None:
            self.viewport.show_snapshot(self.snapshot, selected_id=entity_id)
        self.statusBar().showMessage("Face selected · Enter to assign, I=inlet, O=outlet, W=wall")

    def _solid_containing_face(self, face_id: int) -> int | None:
        for entity in self.document.entities:
            if entity.type_name.upper() not in {"MANIFOLD_SOLID_BREP", "BREP_WITH_VOIDS"}:
                continue
            pending = list(entity.references)
            seen = set()
            while pending:
                entity_id = pending.pop()
                if entity_id == face_id:
                    return entity.entity_id
                if entity_id in seen:
                    continue
                seen.add(entity_id)
                child = self.document.entity(entity_id)
                if child is not None:
                    pending.extend(child.references)
        return None

    def select_opening_edge(self, entity_id: int) -> None:
        if self.document is None or self.snapshot is None or self.geometry_builder is None:
            return
        try:
            self.selected_entity_id = entity_id
            loop = self._find_planar_loop(entity_id)
            self.opening_points = loop
            self.selected_face = None
            self._show_cap(loop)
            self.viewport.show_snapshot(self.snapshot, selected_id=entity_id)
            self.face_label.setText(f"Opening loop · {len(loop)} points · assign inlet or outlet")
            self.statusBar().showMessage("Closed planar opening found · choose inlet/outlet and press Enter")
        except ValueError as error:
            self.opening_points = None
            self._remove_cap()
            self.face_label.setText(f"No usable opening loop: {error}")
            self.statusBar().showMessage("Could not create an opening cap")

    def focus_selection(self) -> None:
        if self.selected_entity_id is None or not self.viewport.focus_entity(self.selected_entity_id):
            self.statusBar().showMessage("Select a face, edge, or vertex to focus")

    def _find_planar_loop(self, selected_id: int):
        document = self.document
        point_map = self.snapshot.points
        lines = {line.entity_id: line.points for line in self.snapshot.polylines}
        candidates = []
        for loop_entity in document.entities:
            if loop_entity.type_name.upper() != "EDGE_LOOP":
                continue
            loop_aggregate = next(
                (arg for arg in loop_entity.arguments if isinstance(arg, StepAggregate)), None
            )
            if loop_aggregate is None:
                continue
            oriented_ids = [item.entity_id for item in loop_aggregate.values if isinstance(item, StepReference)]
            oriented_edges = []
            contains_selection = False
            for oriented_id in oriented_ids:
                oriented = document.entity(oriented_id)
                if oriented is None or oriented.type_name.upper() != "ORIENTED_EDGE":
                    continue
                refs = list(oriented.references)
                if len(refs) < 3:
                    continue
                start_vertex, end_vertex, edge_id = refs[0], refs[1], refs[-1]
                contains_selection |= selected_id in {oriented_id, edge_id}
                oriented_edges.append((start_vertex, end_vertex, edge_id))
            if not contains_selection or len(oriented_edges) < 3:
                continue
            if any(oriented_edges[i][1] != oriented_edges[(i + 1) % len(oriented_edges)][0]
                   for i in range(len(oriented_edges))):
                continue

            polygon = []
            for start_id, end_id, edge_id in oriented_edges:
                start = self._vertex_point(start_id, point_map)
                end = self._vertex_point(end_id, point_map)
                samples = lines.get(edge_id, ())
                if len(samples) >= 2:
                    coords = [(p.x, p.y, p.z) for p in samples]
                    if self._distance(coords[0], start) + self._distance(coords[-1], end) > \
                            self._distance(coords[-1], start) + self._distance(coords[0], end):
                        coords.reverse()
                    polygon.extend(coords[:-1])
                else:
                    polygon.append(start)
            if len(polygon) >= 3 and self._is_planar(polygon):
                candidates.append((abs(self._polygon_area(polygon)), polygon))
        if not candidates:
            raise ValueError("click an edge on a closed, planar aperture loop")
        return min(candidates, key=lambda item: item[0])[1]

    def _vertex_point(self, vertex_id: int, point_map):
        vertex = self.document.entity(vertex_id)
        point_id = vertex.references[-1] if vertex and vertex.references else None
        point = point_map.get(point_id)
        if point is None:
            raise ValueError("the loop has a vertex without a usable point")
        return (point.x, point.y, point.z)

    @staticmethod
    def _distance(a, b):
        return sqrt(sum((a[i] - b[i]) ** 2 for i in range(3)))

    @staticmethod
    def _polygon_area(points):
        normal = [0.0, 0.0, 0.0]
        for i, point in enumerate(points):
            nxt = points[(i + 1) % len(points)]
            normal[0] += (point[1] - nxt[1]) * (point[2] + nxt[2])
            normal[1] += (point[2] - nxt[2]) * (point[0] + nxt[0])
            normal[2] += (point[0] - nxt[0]) * (point[1] + nxt[1])
        return sqrt(sum(value * value for value in normal)) / 2

    def _is_planar(self, points):
        origin = points[0]
        normal = None
        for i in range(1, len(points) - 1):
            a = tuple(points[i][j] - origin[j] for j in range(3))
            b = tuple(points[i + 1][j] - origin[j] for j in range(3))
            cross = (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])
            length = sqrt(sum(value * value for value in cross))
            if length > 1e-12:
                normal = tuple(value / length for value in cross)
                break
        if normal is None:
            return False
        scale = max(self._distance(origin, point) for point in points)
        return all(abs(sum(normal[j] * (p[j] - origin[j]) for j in range(3))) <= max(scale * 1e-6, 1e-7)
                   for p in points)

    def _show_cap(self, points):
        self._remove_cap()
        vtk_points = vtk.vtkPoints()
        for point in points:
            vtk_points.InsertNextPoint(*point)
        cells = vtk.vtkCellArray()
        for i in range(1, len(points) - 1):
            triangle = vtk.vtkTriangle()
            for j, point_id in enumerate((0, i, i + 1)):
                triangle.GetPointIds().SetId(j, point_id)
            cells.InsertNextCell(triangle)
        data = vtk.vtkPolyData()
        data.SetPoints(vtk_points)
        data.SetPolys(cells)
        mapper = vtk.vtkPolyDataMapper()
        mapper.SetInputData(data)
        self.cap_actor = vtk.vtkActor()
        self.cap_actor.SetMapper(mapper)
        self.cap_actor.GetProperty().SetColor(0.15, 0.85, 0.72)
        self.cap_actor.GetProperty().SetOpacity(0.42)
        self.cap_actor.GetProperty().EdgeVisibilityOn()
        self.cap_actor.GetProperty().SetEdgeColor(0.15, 1.0, 0.78)
        self.cap_actor.SetPickable(False)
        if self.viewport.renderer is not None:
            self.viewport.renderer.AddActor(self.cap_actor)
            self.viewport.widget.GetRenderWindow().Render()

    def _remove_cap(self):
        if self.cap_actor is not None and self.viewport.renderer is not None:
            self.viewport.renderer.RemoveActor(self.cap_actor)
            self.viewport.widget.GetRenderWindow().Render()
        self.cap_actor = None

    def assign_quick(self, role: str) -> None:
        if self.selected_face is not None:
            self.role.setCurrentText(role)
            self.assign_role()

    def assign_role(self) -> None:
        if self.selected_face is None and self.opening_points is None:
            return
        role = self.role.currentText()
        if self.opening_points is not None and role not in {"inlet", "outlet"}:
            QMessageBox.information(self, "Opening role", "An opening cap can be tagged as inlet or outlet.")
            return
        if self.selected_face is not None:
            self.face_roles[self.selected_face] = role
        elif self.opening_points is not None:
            self.face_roles[-len(self.face_roles) - 1] = role
            self.cap_actor.GetProperty().SetColor(0.15, 0.55, 1.0 if role == "inlet" else 0.3)
            self.viewport.widget.GetRenderWindow().Render()
        self._refresh_roles()
        self.mark_dirty()
        self.face_label.setText(f"Opening cap · {role}" if self.opening_points is not None else
                                f"STEP face #{self.selected_face} · {role}")

    def _refresh_roles(self) -> None:
        if not self.face_roles:
            self.roles_label.setText("No boundary faces assigned")
            return
        counts: dict[str, int] = {}
        for role in self.face_roles.values():
            counts[role] = counts.get(role, 0) + 1
        self.roles_label.setText("Assigned faces\n" + "\n".join(f"{name}: {count}" for name, count in counts.items()))

    def closeEvent(self, event) -> None:
        if self.dirty:
            choice = QMessageBox(self)
            choice.setWindowTitle("Unsaved workspace")
            choice.setText("This workspace has unsaved changes. Save before closing?")
            choice.setStandardButtons(
                QMessageBox.StandardButton.Save
                | QMessageBox.StandardButton.Discard
                | QMessageBox.StandardButton.Cancel
            )
            choice.setDefaultButton(QMessageBox.StandardButton.Save)
            answer = choice.exec()
            if answer == QMessageBox.StandardButton.Cancel:
                event.ignore()
                return
            if answer == QMessageBox.StandardButton.Save and not self.save_workspace():
                event.ignore()
                return
        if getattr(self, "welcome_window", None) is not None:
            self.welcome_window.show()
        super().closeEvent(event)


class WelcomeWindow(QMainWindow):
    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle("OpenFlowSim · Welcome")
        self.resize(900, 680)
        self.setMinimumSize(700, 520)
        self.editor_windows: list[OpenFlowWindow] = []

        root = QWidget()
        layout = QVBoxLayout(root)
        layout.setContentsMargins(42, 36, 42, 32)
        layout.setSpacing(18)
        heading = QLabel("Welcome to OpenFlowSim")
        heading.setStyleSheet("font-size: 30px; font-weight: 700;")
        layout.addWidget(heading)
        subtitle = QLabel("Open a recent workspace or start a new flow setup.")
        subtitle.setObjectName("muted")
        layout.addWidget(subtitle)

        actions = QHBoxLayout()
        self.new_button = QPushButton("＋  New Workspace")
        self.new_button.setObjectName("newWorkspaceButton")
        self.new_button.clicked.connect(self.new_workspace)
        actions.addWidget(self.new_button)
        actions.addStretch(1)
        layout.addLayout(actions)

        recent_heading = QLabel("Recent workspaces")
        recent_heading.setStyleSheet("font-size: 18px; font-weight: 600; margin-top: 10px;")
        layout.addWidget(recent_heading)
        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.scroll.setFrameShape(QFrame.Shape.NoFrame)
        self.recent_content = QWidget()
        self.recent_layout = QVBoxLayout(self.recent_content)
        self.recent_layout.setContentsMargins(0, 0, 8, 0)
        self.recent_layout.setSpacing(10)
        self.scroll.setWidget(self.recent_content)
        layout.addWidget(self.scroll, 1)

        self.setCentralWidget(root)
        self.setStyleSheet(STYLE + """
QPushButton#newWorkspaceButton { background: #3b82f6; border-color: #3b82f6;
  color: white; font-size: 15px; font-weight: 650; padding: 12px 18px; }
QPushButton#newWorkspaceButton:hover { background: #5594fa; }
QFrame#recentCard { background: #202630; border: 1px solid #303b49; border-radius: 10px; }
QLabel#preview { background: #151a21; border: 1px solid #374252; border-radius: 6px; }
""")
        self.refresh_recent()

    @staticmethod
    def settings() -> QSettings:
        return QSettings("OpenFlowSim", "OpenFlowSim")

    @classmethod
    def record_recent(cls, workspace_dir: Path) -> None:
        paths = [str(workspace_dir.resolve())]
        paths.extend(path for path in cls.settings().value("recent_workspaces", [], type=list)
                     if path != paths[0] and Path(path).is_dir())
        cls.settings().setValue("recent_workspaces", paths[:12])

    def refresh_recent(self) -> None:
        while self.recent_layout.count():
            item = self.recent_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        paths = self.settings().value("recent_workspaces", [], type=list)
        paths = [Path(path) for path in paths if Path(path).is_dir()]
        if not paths:
            empty = QLabel("No saved workspaces yet. Create one, import a STEP scene, and save it.")
            empty.setObjectName("muted")
            empty.setMinimumHeight(110)
            self.recent_layout.addWidget(empty)
        for path in paths:
            self.recent_layout.addWidget(self._recent_card(path))
        self.recent_layout.addStretch(1)

    def _recent_card(self, path: Path) -> QWidget:
        card = QFrame()
        card.setObjectName("recentCard")
        row = QHBoxLayout(card)
        row.setContentsMargins(12, 10, 14, 10)
        preview = QLabel()
        preview.setObjectName("preview")
        preview.setFixedSize(200, 100)
        image_path = path / "preview.png"
        pixmap = QPixmap(str(image_path)) if image_path.is_file() else QPixmap()
        if not pixmap.isNull():
            preview.setPixmap(pixmap.scaled(200, 100, Qt.AspectRatioMode.IgnoreAspectRatio,
                                            Qt.TransformationMode.SmoothTransformation))
        else:
            preview.setText("No preview yet")
            preview.setAlignment(Qt.AlignmentFlag.AlignCenter)
            preview.setStyleSheet("color: #788596;")
        row.addWidget(preview)
        text_column = QVBoxLayout()
        name = QLabel(path.name)
        name.setStyleSheet("font-size: 16px; font-weight: 650;")
        text_column.addWidget(name)
        project = path / "workspace.ofs"
        detail = QLabel(str(path))
        detail.setObjectName("muted")
        detail.setWordWrap(True)
        text_column.addWidget(detail)
        text_column.addStretch(1)
        row.addLayout(text_column, 1)
        open_button = QPushButton("Open")
        open_button.clicked.connect(lambda _checked=False, selected=path: self.open_workspace(selected))
        row.addWidget(open_button)
        card.setToolTip(str(project))
        return card

    def new_workspace(self) -> None:
        default_root = REPO_ROOT / "workspaces"
        default_root.mkdir(parents=True, exist_ok=True)
        chosen = QFileDialog.getExistingDirectory(self, "Choose workspace location", str(default_root))
        if not chosen:
            return
        selected = Path(chosen).resolve()
        default_root = default_root.resolve()
        if selected == default_root:
            name, accepted = QInputDialog.getText(self, "New Workspace", "Workspace name:")
            name = name.strip()
            if not accepted or not name:
                return
            if any(char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for char in name):
                QMessageBox.warning(self, "Invalid name", "Use letters, numbers, hyphens, or underscores.")
                return
            destination = selected / name
            if destination.exists():
                QMessageBox.warning(self, "Workspace exists", f"{destination} already exists.")
                return
            destination.mkdir(parents=True)
        else:
            if not selected.is_dir() or any(selected.iterdir()):
                QMessageBox.warning(
                    self,
                    "Choose an empty folder",
                    f"Please select an empty folder, or choose the default workspace folder:\n{default_root}",
                )
                return
            destination = selected
        if not destination.is_dir():
            QMessageBox.warning(self, "Workspace folder unavailable", f"Could not use {destination}.")
            return
        self.open_editor(destination)

    def open_workspace(self, workspace_dir: Path) -> None:
        project_path = workspace_dir / "workspace.ofs"
        if not project_path.is_file():
            QMessageBox.warning(self, "Workspace not saved", "This workspace has no workspace.ofs file yet.")
            return
        try:
            editor = self.open_editor(workspace_dir)
            editor.load_workspace(project_path)
            self.record_recent(workspace_dir)
        except Exception as error:
            QMessageBox.critical(self, "Could not open workspace", str(error))

    def open_editor(self, workspace_dir: Path) -> OpenFlowWindow:
        editor = OpenFlowWindow(workspace_dir)
        editor.welcome_window = self
        self.editor_windows.append(editor)
        self.hide()
        editor.show()
        return editor


def main() -> None:
    app = QApplication(sys.argv)
    app.setStyle("Fusion")
    window = WelcomeWindow()
    window.show()
    raise SystemExit(app.exec())


if __name__ == "__main__":
    main()
