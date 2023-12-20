use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::Duration,
};

use eframe::egui;
use egui::{menu, Color32, ProgressBar, Widget};
use egui_plot::{CoordinatesFormatter, Corner, Legend, Line, Plot, PlotPoint, PlotPoints};
use fxhash::FxHashMap;
use log::{error, info};
use nix::libc::remove;
use std::io::{BufRead, BufReader, Error, Write};
use walkdir::WalkDir;

use crate::{EguiUpdate, RecordingCommand, RecordingPlayback};

pub fn start_egui(
    packet_hq_command_sender: rtrb::Producer<RecordingCommand>,
    egui_update_receiver: rtrb::Consumer<EguiUpdate>,
) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(620.0, 440.0)),
        ..Default::default()
    };
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Box::new(EguiApp::new(packet_hq_command_sender, egui_update_receiver))
        }),
    )
}
#[derive(Clone)]
struct LineData {
    points: Vec<[f64; 2]>,
    name: String,
    color: Color32,
}
impl Into<Line> for LineData {
    fn into(self) -> Line {
        Line::new(PlotPoints::new(self.points))
            .color(self.color)
            .name(self.name)
    }
}
struct PlotData {
    lines: Vec<LineData>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistentSettings {
    folder_to_load: Option<PathBuf>,
}
impl Default for PersistentSettings {
    fn default() -> Self {
        Self {
            folder_to_load: Default::default(),
        }
    }
}

struct EguiApp {
    packet_hq_command_sender: rtrb::Producer<RecordingCommand>,
    egui_update_receiver: rtrb::Consumer<EguiUpdate>,
    recordings: Vec<RecordingPlayback>,
    plots: FxHashMap<String, PlotData>,
    active_programs: Vec<String>,
    current_intensity: u32,
    all_program_values: Vec<String>,
    selected_program_value: String,
    persistent_settings: PersistentSettings,
}
impl EguiApp {
    pub fn new(
        packet_hq_command_sender: rtrb::Producer<RecordingCommand>,
        egui_update_receiver: rtrb::Consumer<EguiUpdate>,
    ) -> Self {
        let persistent_settings = {
            if let Ok(file) = std::fs::read_to_string("./settings.json") {
                if let Ok(settings) = serde_json::from_str(&file) {
                    settings
                } else {
                    PersistentSettings::default()
                }
            } else {
                PersistentSettings::default()
            }
        };
        let mut s = Self {
            packet_hq_command_sender,
            egui_update_receiver,
            recordings: vec![],
            plots: FxHashMap::default(),
            active_programs: vec![],
            current_intensity: 0,
            all_program_values: vec![],
            selected_program_value: String::new(),
            persistent_settings,
        };
        s.apply_persistent_settings();
        s
    }
    fn apply_persistent_settings(&mut self) {
        if let Some(path) = &self.persistent_settings.folder_to_load {
            // Load folder
            self.load_folder(path.clone());
        }
    }
    fn save_persistent_settings(&self) {
        let Ok(json) = serde_json::to_string(&self.persistent_settings) else {
            error!("Failed to turn settings into JSON");
            return;
        };
        let Ok(mut output) = std::fs::File::create("./settings.json") else {
        error!("Failed to open settings file");
        return;
    };
        write!(output, "{json}").ok();
    }
    pub fn load_folder(&mut self, folder: PathBuf) {
        self.persistent_settings.folder_to_load = Some(folder.clone());
        self.save_persistent_settings();
        for entry in WalkDir::new(folder).into_iter().filter_map(|e| e.ok()) {
            match entry.path().extension().and_then(OsStr::to_str) {
                Some("postcard") => {
                    if let Ok(recording_playback) =
                        RecordingPlayback::from_file(&entry.path().to_path_buf())
                    {
                        self.recordings.push(recording_playback);
                    };
                }
                _ => (),
            }
        }
        if let Err(e) = self
            .packet_hq_command_sender
            .push(RecordingCommand::ReplaceAllRecordings(
                self.recordings.clone(),
            ))
        {
            error!("Failed to send recordings to PacketHQ: {e}");
        }
    }
    pub fn update_playing_recordings(&mut self) {
        // For all main_programs, collect the available intensities and choose the one that is closest
        for active_program in &self.active_programs {
            let mut available_intensities = vec![];
            for r in &self.recordings {
                if r.recorded_packets.main_program == *active_program {
                    available_intensities.push(r.recorded_packets.intensity);
                }
            }
            if available_intensities.len() > 0 {
                let mut best_distance = i32::MAX;
                let mut closest_intensity = 0;
                for i in available_intensities {
                    let distance = (i as i32 - self.current_intensity as i32).abs();
                    if distance < best_distance {
                        closest_intensity = i;
                        best_distance = distance;
                    }
                }
                // Activate the selected recording and stop any playing recordings that aren't playing
                for r in &mut self.recordings {
                    if r.recorded_packets.main_program == *active_program {
                        if r.recorded_packets.intensity == closest_intensity {
                            if !r.playing {
                                r.playing = true;
                                if let Err(e) = self.packet_hq_command_sender.push(
                                    RecordingCommand::StartPlayback(
                                        r.recorded_packets.name.clone(),
                                    ),
                                ) {
                                    error!("Failed to send command to PacketHq: {e}");
                                }
                            }
                        } else if r.playing {
                            r.playing = false;
                            if let Err(e) =
                                self.packet_hq_command_sender
                                    .push(RecordingCommand::StopPlayback(
                                        r.recorded_packets.name.clone(),
                                    ))
                            {
                                error!("Failed to send command to PacketHq: {e}");
                            }
                        }
                    }
                }
            }
        }
        // Stop recordings in an inactive program
        for r in &mut self.recordings {
            if r.playing {
                if !self
                    .active_programs
                    .contains(&r.recorded_packets.main_program)
                {
                    r.playing = false;
                    if let Err(e) =
                        self.packet_hq_command_sender
                            .push(RecordingCommand::StopPlayback(
                                r.recorded_packets.name.clone(),
                            ))
                    {
                        error!("Failed to send command to PacketHq: {e}");
                    }
                }
            }
        }
    }
    pub fn add_active_program(&mut self, new_active_program: String) {
        if !self.active_programs.contains(&new_active_program) {
            // // Start any recordings that match the new active program
            // for r in &mut self.recordings {
            //     if !r.playing && r.recorded_packets.main_program == new_active_program {
            //         info!("Starting {}", &r.recorded_packets.name);
            //         // TODO: Get the best match for intensity setting
            //         r.playing = true;
            //         if let Err(e) =
            //             self.packet_hq_command_sender
            //                 .push(RecordingCommand::StartPlayback(
            //                     r.recorded_packets.name.clone(),
            //                 ))
            //         {
            //             error!("Failed to send command to PacketHq: {e}");
            //         }
            //     }
            // }
            self.active_programs.push(new_active_program);
        }
        self.update_playing_recordings();
    }
    /// After removing an "active program" from the list, stop any recordings matching it
    pub fn stop_active_program(&mut self, removed_active_program: String) {
        for r in &mut self.recordings {
            if r.playing && r.recorded_packets.main_program == removed_active_program {
                if let Err(e) = self
                    .packet_hq_command_sender
                    .push(RecordingCommand::StopPlayback(
                        r.recorded_packets.name.clone(),
                    ))
                {
                    error!("Failed to send command to PacketHq: {e}");
                }
            }
        }
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Receive messages
        while let Ok(update) = self.egui_update_receiver.pop() {
            match update {
                EguiUpdate::AllRecordings(recordings) => self.recordings = recordings,
                EguiUpdate::StartingPlaybackOfRecording(name) => {
                    if let Some(r) = self
                        .recordings
                        .iter_mut()
                        .find(|r| r.recorded_packets.name == name)
                    {
                        r.playing = true;
                    }
                }
                EguiUpdate::StoppingPlaybackOfRecording(name) => {
                    if let Some(r) = self
                        .recordings
                        .iter_mut()
                        .find(|r| r.recorded_packets.name == name)
                    {
                        r.playing = false;
                    }
                }
                EguiUpdate::PlaybackUpdate(name, playback_data) => {
                    if let Some(r) = self
                        .recordings
                        .iter_mut()
                        .find(|r| r.recorded_packets.name == name)
                    {
                        r.playing = playback_data.playing;
                        r.current_duration = playback_data.current_timestamp;
                        r.current_packet = playback_data.current_index;
                    }
                }
            }
        }

        egui::SidePanel::left("global_actions").show(ctx, |ui| {
            ui.heading("Intensity setting");
            let mut new_intensity = self.current_intensity;
            egui::DragValue::new(&mut new_intensity).ui(ui);
            if new_intensity != self.current_intensity {
                self.current_intensity = new_intensity;
                self.update_playing_recordings();
            }
            ui.heading("Active programs");
            egui::Grid::new("active_programs")
                .num_columns(2)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    let mut index_to_remove = None;
                    for (i, program) in self.active_programs.iter().enumerate() {
                        ui.label(program);
                        if ui.button("x").clicked() {
                            index_to_remove = Some(i);
                        }
                        ui.end_row();
                    }
                    if let Some(i) = index_to_remove {
                        let removed_program = self.active_programs.remove(i);
                        self.stop_active_program(removed_program);
                    }
                });
            egui::ComboBox::from_label("")
                .selected_text(format!("{:?}", &self.selected_program_value))
                .show_ui(ui, |ui| {
                    for possible_program in &self.all_program_values {
                        ui.selectable_value(
                            &mut self.selected_program_value,
                            possible_program.clone(),
                            possible_program,
                        );
                    }
                });
            if ui.button("Add active program").clicked() {
                self.add_active_program(self.selected_program_value.clone());
            }
            ui.separator();
            if ui.button("Analyse metadata").clicked() {
                // Set main program for all recordings
                for rec in &mut self.recordings {
                    rec.recorded_packets.analyse_main_program();
                }
                // Calculate the mean intensity per recording and assign ascending number within each program category
                analyse_recording_intensities(&mut self.recordings);
            }
        });

        egui::TopBottomPanel::top("my_panel").show(ctx, |ui| {
            menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Generate plots").clicked() {
                        generate_plot_data(&self.recordings, &mut self.plots);
                    }
                    if ui.button("Load folder").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            info!("Loading from folder {folder:?}");
                            self.load_folder(folder);
                        }
                    }
                    if ui.button("Save all to folder").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            info!("Saving to folder {path:?}");
                            for rec in &self.recordings {
                                let mut file_path = path.clone();
                                file_path.push(&format!("{}.postcard", &rec.recorded_packets.name));

                                if let Err(e) = rec.recorded_packets.save_postcard(&file_path) {
                                    error!("Failed to save recording: {e} to path {file_path:?}");
                                }
                            }
                        }
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("sys|calls data recording editor");
            let mut recordings_to_remove = vec![];
            for (i, recording) in self.recordings.iter_mut().enumerate() {
                let plot = self.plots.get(&recording.recorded_packets.name);
                if let Some(command) = recording_window(ctx, recording, plot) {
                    if matches!(command, RecordingCommand::CloseRecording(_)) {
                        recordings_to_remove.push(i)
                    }
                    self.packet_hq_command_sender.push(command).ok();
                }
            }
            for i in recordings_to_remove.iter().rev() {
                self.recordings.remove(*i);
            }
        });
        self.all_program_values.clear();
        for r in &self.recordings {
            if !self
                .all_program_values
                .contains(&r.recorded_packets.main_program)
                && !self
                    .active_programs
                    .contains(&r.recorded_packets.main_program)
            {
                self.all_program_values
                    .push(r.recorded_packets.main_program.clone());
            }
        }
        if self.selected_program_value == "" && self.all_program_values.len() > 0 {
            self.selected_program_value = self.all_program_values.first().unwrap().clone();
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_persistent_settings();
    }
}

fn generate_plot_data(recordings: &[RecordingPlayback], plots: &mut FxHashMap<String, PlotData>) {
    let timestep = Duration::from_millis(10);
    for rec in recordings {
        let mut time = Duration::ZERO;
        let mut plot_data = PlotData { lines: vec![] };
        let mut line = LineData {
            points: vec![],
            name: String::from("none"),
            color: Color32::RED,
        };
        let mut current_i = 0;

        while time < rec.last_packet_timestamp {
            let mut calls_per_ts = 0;
            while rec.recorded_packets.records[current_i].timestamp < time {
                calls_per_ts += 1;
                current_i += 1;
            }
            line.points.push([time.as_secs_f64(), calls_per_ts as f64]);
            time += timestep;
        }
        plot_data.lines = vec![line];
        plots.insert(rec.recorded_packets.name.clone(), plot_data);
    }
}

fn recording_window(
    ctx: &egui::Context,
    recording: &mut RecordingPlayback,
    plot: Option<&PlotData>,
) -> Option<RecordingCommand> {
    let mut command = None;
    egui::Window::new(&recording.recorded_packets.name)
        // .open(true)
        .resizable(true)
        .default_width(280.0)
        .show(ctx, |ui| {
            egui::Grid::new("recording data")
                .num_columns(2)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut recording.recorded_packets.name);
                    ui.end_row();
                    ui.label("Playing:");
                    ui.label(if recording.playing { "yes" } else { "no" });
                    ui.end_row();
                    ui.label("Playhead duration:");
                    ui.label(&humantime::format_duration(recording.current_duration).to_string());
                    ui.end_row();
                    ui.label("Last packet timestamp:");
                    ui.label(
                        &humantime::format_duration(recording.last_packet_timestamp).to_string(),
                    );
                    ui.end_row();
                    ui.label("Intensity:");
                    ui.add(
                        egui::DragValue::new(&mut recording.recorded_packets.intensity).speed(1.0),
                    );
                    ui.end_row();
                    ui.label("Main_program:");
                    ui.text_edit_singleline(&mut recording.recorded_packets.main_program);
                });
            if ui.button("Trim start silence").clicked() {
                recording.trim_silence_before();
                command = Some(RecordingCommand::ReplaceRecording(recording.clone()));
            }
            if ui.button("Play").clicked() {
                recording.playing = true;
                command = Some(RecordingCommand::StartPlayback(
                    recording.recorded_packets.name.clone(),
                ));
            }
            if ui.button("Stop").clicked() {
                recording.playing = false;
                command = Some(RecordingCommand::StopPlayback(
                    recording.recorded_packets.name.clone(),
                ));
            }
            if ui.button("Split into programs").clicked() {
                todo!()
            }
            if ui.button("Get main program").clicked() {
                recording.recorded_packets.analyse_main_program();
                command = Some(RecordingCommand::ReplaceRecording(recording.clone()));
            }
            if ui.button("Close").clicked() {
                command = Some(RecordingCommand::CloseRecording(
                    recording.recorded_packets.name.clone(),
                ));
            }
            let progress = recording.current_duration.as_secs_f32()
                / recording.last_packet_timestamp.as_secs_f32();
            ProgressBar::new(progress)
                .show_percentage()
                .animate(recording.playing)
                .ui(ui);
            if let Some(plot_date) = plot {
                let mut plot = Plot::new("recording plot")
                    .legend(Legend::default())
                    .y_axis_width(4)
                    .show_axes(true)
                    .show_grid(true);
                // if self.square {
                //     plot = plot.view_aspect(1.0);
                // }
                // if self.proportional {
                //     plot = plot.data_aspect(1.0);
                // }
                plot =
                    plot.coordinates_formatter(Corner::LeftBottom, CoordinatesFormatter::default());
                plot.show(ui, |plot_ui| {
                    for line_data in &plot_date.lines {
                        plot_ui.line(((*line_data).clone()).into());
                    }
                });
                // .response
            }
        });
    command
}

fn analyse_recording_intensities(recordings: &mut [RecordingPlayback]) {
    let timestep = Duration::from_millis(50);
    let mut recording_activity = FxHashMap::default();
    for (i, rec) in recordings.iter_mut().enumerate() {
        let activity = rec.recorded_packets.mean_activity(timestep);
        let entry = recording_activity
            .entry(rec.recorded_packets.main_program.clone())
            .or_insert(Vec::new());
        entry.push((activity, i));
    }
    for list in recording_activity.values_mut() {
        list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for (intensity, (_activity, index)) in list.iter().enumerate() {
            recordings[*index].recorded_packets.intensity = intensity as u32;
        }
    }
}