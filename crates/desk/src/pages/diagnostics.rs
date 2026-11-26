//! Settings › Diagnostics, as t3code's: the processes Ghost runs, as a
//! tree (agents, what they started, your terminals) with their CPU and
//! memory and this Mac's load, each stopped from here; and the warnings
//! and errors in Ghost's log.

use gpui::component::button::ButtonVariants as _;
use gpui::component::{
    list::ListItem,
    setting::{SettingGroup, SettingItem},
    tree::{TreeItem, tree},
};
use gpui::{App, Context, IntoElement, SharedString, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::{
    desk::Desk,
    diagnostics::Proc,
    fonts::Faces,
    theme,
    ui::{self, Weight},
};

/// How high a process's row is in the tree.
const ROW: f32 = 44.;

/// The processes, in a tree's order (each with its depth), as gpui-kit's
/// tree takes them: each under the one before it that is less deep.
pub(crate) fn process_tree(procs: &[Proc]) -> Vec<TreeItem> {
    fn under(procs: &[Proc], at: &mut usize, depth: usize) -> Vec<TreeItem> {
        let mut out = Vec::new();
        while let Some(p) = procs.get(*at).filter(|p| p.depth == depth) {
            *at += 1;
            let children = under(procs, at, depth + 1);
            out.push(
                TreeItem::new(p.pid.to_string(), p.name.clone())
                    .children(children)
                    .expanded(true),
            );
        }
        out
    }
    let first = procs.first().map_or(1, |p| p.depth);
    under(procs, &mut 0, first)
}

impl Desk {
    /// The Diagnostics tab: this Mac, Ghost's processes, the log's troubles.
    pub(super) fn diagnostics(&self, w: &WeakEntity<Desk>) -> Vec<SettingGroup> {
        let processes = &self.pages.processes.1;
        let memory = crate::tidy::shown(processes.memory);
        let held = crate::tidy::shown(processes.procs.iter().map(|p| p.kb * 1024).sum());
        let mac = format!(
            "Load {} on {} cores, {memory} of memory; Ghost's processes hold {held}.",
            processes.load, processes.cores
        );
        let words = &["processes", "diagnostics", "cpu", "memory"];
        let shown = if processes.procs.is_empty() {
            item(w, "Processes", "Ghost runs nothing else now.", |_, _, _| {
                div().into_any_element()
            })
        } else {
            whole(w, words, |d, faces, cx| {
                d.processes(faces, cx).into_any_element()
            })
        };
        let log: Vec<SettingItem> = if self.pages.troubles.is_empty() {
            vec![item(w, "Nothing went wrong lately.", "", |_, _, _| {
                div().into_any_element()
            })]
        } else {
            (self.pages.troubles.iter().cloned())
                .map(|line| {
                    whole(w, &["log", "errors"], move |_, faces, _| {
                        let color = if line.contains(" ERROR ") {
                            theme::coral()
                        } else {
                            theme::text2()
                        };
                        ui::fine_mono(faces, line.clone())
                            .text_color(color)
                            .into_any_element()
                    })
                })
                .collect()
        };
        vec![
            group(
                "Ghost",
                vec![item(w, "Ghost", mac, |_, _, cx| {
                    ui::button("diagnostics-read", "Read again", Weight::Secondary)
                        .on_click(cx.listener(|this, _, _, _| this.diagnose(this.pages.on)))
                        .into_any_element()
                })],
            ),
            group("Processes", vec![shown]),
            group("Warnings and errors in the log", log),
        ]
    }

    /// Ghost's processes as gpui-kit's tree: each with its CPU and memory,
    /// an agent let go and any other stopped.
    fn processes(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let desk = cx.entity();
        let faces = faces.clone();
        let rows = self.pages.processes.1.procs.len();
        div().w_full().h(px(ROW * rows as f32)).child(tree(
            &self.pages.process_tree,
            move |ix, entry, _, _, cx: &mut App| {
                let pid: u32 = entry.item().id.parse().unwrap_or_default();
                let (agents, processes) = &desk.read(cx).pages.processes;
                let p = processes.procs.iter().find(|p| p.pid == pid).cloned();
                let agent = (agents.iter())
                    .find(|(_, id)| *id == pid)
                    .map(|(a, _)| a.clone());
                let name = match &agent {
                    Some(a) => desk.read(cx).name_of(a),
                    None => entry.item().label.to_string(),
                };
                let about = p.map_or_else(String::new, |p| {
                    format!(
                        "{pid} · {:.1}% CPU · {}",
                        p.cpu,
                        crate::tidy::shown(p.kb * 1024)
                    )
                });
                let stop = desk.clone();
                let control = match agent {
                    Some(id) => ui::button(
                        SharedString::from(format!("let-go-{ix}")),
                        "Let it go",
                        Weight::Secondary,
                    )
                    .on_click(move |_, _, cx| {
                        stop.update(cx, |this, cx| {
                            this.let_go(this.pages.on, &id);
                            this.weigh_agents(this.pages.on);
                            cx.notify();
                        });
                    }),
                    None => ui::button(
                        SharedString::from(format!("stop-{ix}")),
                        "Stop",
                        Weight::Secondary,
                    )
                    .danger()
                    .on_click(move |_, _, cx| {
                        let d = stop.read(cx);
                        d.stop_process(d.pages.on, pid);
                    }),
                };
                ListItem::new(ix)
                    .h(px(ROW))
                    .pl(px(4. + 18. * entry.depth() as f32))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(ui::described(&faces, name, Some(about)).flex_1())
                            .child(control),
                    )
            },
        ))
    }
}
