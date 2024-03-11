// Copyright (C) Pavel Grebnev 2023-2024
// Distributed under the MIT License (license terms are at http://opensource.org/licenses/MIT).

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use iced::alignment::{self, Alignment};
use iced::theme::{self, Theme};
use iced::widget::pane_grid::{self, Configuration, PaneGrid};
use iced::widget::{
    button, checkbox, column, container, horizontal_space, image, image::Handle, row, scrollable,
    text, text_input, tooltip, vertical_space, Button, Column,
};
use iced::window::{self, request_user_attention, resize};
use iced::{event, executor, keyboard, ContentFit, Event};
use iced::{time, Size};
use iced::{Application, Command, Element, Length, Subscription};
use iced_lazy::responsive;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::mem::swap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{Duration, Instant};

use crate::color_utils;
use crate::config;
use crate::execution;
use crate::execution_lists;
use crate::file_utils;
use crate::string_constants;
use crate::style;
use crate::ui_icons;

const ONE_EXECUTION_LIST_ELEMENT_HEIGHT: u32 = 30;
const ONE_TITLE_LINE_HEIGHT: u32 = 16;
const EMPTY_EXECUTION_LIST_HEIGHT: u32 = 150;
const EXTRA_EDIT_CONTENT_HEIGHT: u32 = 40;

// these should be static not just const
static FILTER_INPUT_ID: Lazy<text_input::Id> = Lazy::new(text_input::Id::unique);
static ARGUMENTS_INPUT_ID: Lazy<text_input::Id> = Lazy::new(text_input::Id::unique);

// caches for visual elements content
pub struct VisualCaches {
    autorerun_count: String,
    is_custom_title_editing: bool,
    recent_logs: Vec<String>,
    icons: ui_icons::IconCaches,
}

pub struct MainWindow {
    panes: pane_grid::State<AppPane>,
    pane_by_pane_type: HashMap<PaneVariant, pane_grid::Pane>,
    execution_data: execution_lists::ExecutionLists,
    app_config: config::AppConfig,
    theme: Theme,
    visual_caches: VisualCaches,
    edit_data: EditData,
    window_state: WindowState,
}

#[derive(Debug, Clone)]
pub struct EditData {
    // a string that is used to filter the list of scripts
    script_filter: String,
    // state of the global to the window editing mode
    window_edit_data: Option<WindowEditData>,
    // do we have unsaved changes
    is_dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EditScriptType {
    ScriptConfig,
    ExecutionList,
}

#[derive(Debug, Clone)]
pub struct EditScriptId {
    idx: usize,
    script_type: EditScriptType,
}

#[derive(Debug, Clone, PartialEq)]
enum ConfigEditType {
    Local,
    Shared,
}

#[derive(Debug, Clone)]
struct WindowEditData {
    is_editing_config: bool,
    edit_type: ConfigEditType,

    // theme color temp strings
    theme_color_background: String,
    theme_color_text: String,
    theme_color_primary: String,
    theme_color_success: String,
    theme_color_danger: String,
    theme_color_caption_text: String,
    theme_color_error_text: String,
}

impl WindowEditData {
    fn from_config(
        config: &config::AppConfig,
        is_editing_config: bool,
        edit_type: ConfigEditType,
    ) -> Self {
        let theme = if let Some(theme) = &get_rewritable_config(&config, &edit_type).custom_theme {
            theme.clone()
        } else {
            config::CustomTheme::default()
        };

        Self {
            is_editing_config,
            edit_type,
            theme_color_background: color_utils::rgb_to_hex(&theme.background),
            theme_color_text: color_utils::rgb_to_hex(&theme.text),
            theme_color_primary: color_utils::rgb_to_hex(&theme.primary),
            theme_color_success: color_utils::rgb_to_hex(&theme.success),
            theme_color_danger: color_utils::rgb_to_hex(&theme.danger),
            theme_color_caption_text: color_utils::rgb_to_hex(&theme.caption_text),
            theme_color_error_text: color_utils::rgb_to_hex(&theme.error_text),
        }
    }
}

struct WindowState {
    pane_focus: Option<pane_grid::Pane>,
    cursor_script: Option<EditScriptId>,
    full_window_size: Size,
    is_command_key_down: bool,
    has_maximized_pane: bool,
}

#[derive(Debug, Clone)]
pub enum WindowMessage {
    WindowResized(Size),
    Clicked(pane_grid::Pane),
    Dragged(pane_grid::DragEvent),
    Resized(pane_grid::ResizeEvent),
    Maximize(pane_grid::Pane, Size),
    Restore,
    MaximizeOrRestoreExecutionPane,
    AddScriptToExecution(config::Guid),
    RunScripts,
    StopScripts,
    ClearExecutionScripts,
    RescheduleScripts,
    Tick(Instant),
    OpenScriptEditing(usize),
    CloseScriptEditing,
    DuplicateConfigScript(EditScriptId),
    RemoveScript(EditScriptId),
    AddScriptToConfig,
    MoveExecutionScriptUp(usize),
    MoveExecutionScriptDown(usize),
    EditScriptName(String),
    EditScriptCommand(String),
    ToggleScriptCommandRelativeToScripter(bool),
    EditScriptWorkingDirectory(String),
    ToggleScriptWorkingDirectoryRelativeToScripter(bool),
    EditScriptIconPath(String),
    ToggleScriptIconPathRelativeToScripter(bool),
    EditArguments(String),
    ToggleRequiresArguments(bool),
    EditArgumentsHint(String),
    EditAutorerunCount(String),
    ToggleIgnoreFailures(bool),
    EnterWindowEditMode,
    ExitWindowEditMode,
    TrySwitchWindowEditMode,
    SaveConfig,
    RevertConfig,
    OpenScriptConfigEditing(usize),
    MoveConfigScriptUp(usize),
    MoveConfigScriptDown(usize),
    ToggleConfigEditing,
    ConfigToggleAlwaysOnTop(bool),
    ConfigToggleWindowStatusReactions(bool),
    ConfigToggleKeepWindowSize(bool),
    ConfigToggleScriptFiltering(bool),
    ConfigToggleTitleEditing(bool),
    ConfigToggleUseCustomTheme(bool),
    ConfigEditThemeBackground(String),
    ConfigEditThemeText(String),
    ConfigEditThemePrimary(String),
    ConfigEditThemeSuccess(String),
    ConfigEditThemeDanger(String),
    ConfigEditThemeCaptionText(String),
    ConfigEditThemeErrorText(String),
    ConfigEditLocalConfigPath(String),
    ConfigToggleLocalConfigPathRelativeToScripter(bool),
    SwitchToSharedConfig,
    SwitchToLocalConfig,
    ToggleScriptHidden(bool),
    CreateCopyOfSharedScript(EditScriptId),
    MoveToShared(EditScriptId),
    SaveAsPreset,
    ScriptFilterChanged(String),
    RequestCloseApp,
    FocusFilter,
    OnCommandKeyStateChanged(bool),
    MoveCursorUp,
    MoveCursorDown,
    MoveScriptDown,
    MoveScriptUp,
    CursorConfirm,
    RemoveCursorScript,
    SwitchPaneFocus(bool),
    SetExecutionListTitleEditing(bool),
    EditExecutionListTitle(String),
    OpenWithDefaultApplication(PathBuf),
    OpenUrl(String),
    SwitchToOriginalSharedScript(EditScriptId),
}

impl Application for MainWindow {
    type Executor = executor::Default;
    type Message = WindowMessage;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<WindowMessage>) {
        let pane_configuration = Configuration::Split {
            axis: pane_grid::Axis::Vertical,
            ratio: 0.25,
            a: Box::new(Configuration::Split {
                axis: pane_grid::Axis::Horizontal,
                ratio: 0.65,
                a: Box::new(Configuration::Pane(AppPane::new(PaneVariant::ScriptList))),
                b: Box::new(Configuration::Pane(AppPane::new(PaneVariant::Parameters))),
            }),
            b: Box::new(Configuration::Split {
                axis: pane_grid::Axis::Vertical,
                ratio: 0.5,
                a: Box::new(Configuration::Pane(AppPane::new(
                    PaneVariant::ExecutionList,
                ))),
                b: Box::new(Configuration::Pane(AppPane::new(PaneVariant::LogOutput))),
            }),
        };
        let panes = pane_grid::State::with_configuration(pane_configuration);

        let mut pane_by_pane_type = HashMap::new();
        for pane in panes.panes.iter() {
            pane_by_pane_type.insert(pane.1.variant.clone(), *pane.0);
        }

        let app_config = config::get_app_config_copy();

        let mut main_window = MainWindow {
            panes,
            pane_by_pane_type,
            execution_data: execution_lists::ExecutionLists::new(),
            theme: get_theme(&app_config, &None),
            app_config,
            visual_caches: VisualCaches {
                autorerun_count: String::new(),
                is_custom_title_editing: false,
                recent_logs: Vec::new(),
                icons: ui_icons::IconCaches::new(),
            },
            edit_data: EditData {
                script_filter: String::new(),
                window_edit_data: None,
                is_dirty: false,
            },
            window_state: WindowState {
                pane_focus: None,
                cursor_script: None,
                full_window_size: Size::new(1024.0, 768.0),
                is_command_key_down: false,
                has_maximized_pane: false,
            },
        };

        update_theme_icons(&mut main_window);
        update_config_cache(&mut main_window.app_config, &main_window.edit_data);

        return (main_window, Command::none());
    }

    fn title(&self) -> String {
        if let Some(window_edit_data) = &self.edit_data.window_edit_data {
            match window_edit_data.edit_type {
                ConfigEditType::Shared if self.app_config.local_config_body.is_some() => {
                    "scripter [Editing shared config]".to_string()
                }
                _ => "scripter [Editing]".to_string(),
            }
        } else if self.execution_data.has_started_execution() {
            if self.execution_data.has_finished_execution() {
                if self.execution_data.has_failed_scripts() {
                    "scripter [Finished with errors]".to_string()
                } else {
                    "scripter [Finished]".to_string()
                }
            } else {
                "scripter [Running]".to_string()
            }
        } else {
            if self.edit_data.is_dirty {
                "scripter [Unsaved changes]".to_string()
            } else {
                "scripter".to_string()
            }
        }
    }

    fn update(&mut self, message: WindowMessage) -> Command<WindowMessage> {
        match message {
            WindowMessage::WindowResized(size) => {
                if !self.window_state.has_maximized_pane {
                    self.window_state.full_window_size = size;
                }
            }
            WindowMessage::Clicked(pane) => {
                self.window_state.pane_focus = Some(pane);
            }
            WindowMessage::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(&split, ratio);
            }
            WindowMessage::Dragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.swap(&pane, &target);
            }
            WindowMessage::Dragged(_) => {}
            WindowMessage::Maximize(pane, window_size) => {
                return maximize_pane(self, pane, window_size);
            }
            WindowMessage::Restore => {
                return restore_window(self);
            }
            WindowMessage::MaximizeOrRestoreExecutionPane => {
                if self.execution_data.has_started_execution() {
                    return if self.window_state.has_maximized_pane {
                        restore_window(self)
                    } else {
                        maximize_pane(
                            self,
                            self.pane_by_pane_type[&PaneVariant::ExecutionList],
                            self.window_state.full_window_size,
                        )
                    };
                } else {
                    if self.window_state.has_maximized_pane {
                        return restore_window(self);
                    }
                }
            }
            WindowMessage::AddScriptToExecution(script_uid) => {
                let is_added = add_script_to_execution(self, script_uid, true);

                if is_added && self.window_state.is_command_key_down {
                    run_scheduled_scripts(self);
                }
            }
            WindowMessage::RunScripts => {
                if !self.edit_data.window_edit_data.is_some() {
                    run_scheduled_scripts(self);
                }
            }
            WindowMessage::StopScripts => {
                if self.execution_data.has_started_execution()
                    && !self.execution_data.has_finished_execution()
                {
                    self.execution_data.request_stop_execution();
                }
            }
            WindowMessage::ClearExecutionScripts => {
                if !self.execution_data.has_started_execution() {
                    clear_edited_scripts(self)
                } else {
                    clear_execution_scripts(self)
                }
            }
            WindowMessage::RescheduleScripts => {
                if self.execution_data.has_started_execution()
                    && self.execution_data.has_finished_execution()
                    && !self.execution_data.is_waiting_execution_to_finish()
                {
                    self.execution_data.reschedule_scripts()
                }
            }
            WindowMessage::Tick(_now) => {
                let has_finished = self.execution_data.tick(&self.app_config);
                if has_finished {
                    if get_rewritable_config_opt(&self.app_config, &self.edit_data.window_edit_data)
                        .window_status_reactions
                    {
                        return request_user_attention(Some(window::UserAttention::Informational));
                    }
                }
            }
            WindowMessage::OpenScriptEditing(script_idx) => {
                select_execution_script(self, script_idx);
            }
            WindowMessage::CloseScriptEditing => {
                clean_script_selection(&mut self.window_state.cursor_script);
            }
            WindowMessage::DuplicateConfigScript(script_id) => {
                match script_id.script_type {
                    EditScriptType::ScriptConfig => match &self.edit_data.window_edit_data {
                        Some(WindowEditData {
                            edit_type: ConfigEditType::Local,
                            ..
                        }) => {
                            if let Some(config) = self.app_config.local_config_body.as_mut() {
                                config.script_definitions.insert(
                                    script_id.idx + 1,
                                    make_script_copy(
                                        config.script_definitions[script_id.idx].clone(),
                                    ),
                                );
                            }
                        }
                        _ => {
                            self.app_config.script_definitions.insert(
                                script_id.idx + 1,
                                make_script_copy(
                                    self.app_config.script_definitions[script_id.idx].clone(),
                                ),
                            );
                        }
                    },
                    EditScriptType::ExecutionList => {}
                };
                if let Some(script) = &mut self.window_state.cursor_script {
                    script.idx = script_id.idx + 1;
                    script.script_type = script_id.script_type;
                }
                update_config_cache(&mut self.app_config, &self.edit_data);
            }
            WindowMessage::RemoveScript(script_id) => remove_script(self, &script_id),
            WindowMessage::AddScriptToConfig => {
                let script = config::OriginalScriptDefinition {
                    uid: config::Guid::new(),
                    name: "new script".to_string(),
                    icon: config::PathConfig::default(),
                    command: config::PathConfig::default(),
                    working_directory: config::PathConfig {
                        path: ".".to_string(),
                        path_type: config::PathType::WorkingDirRelative,
                    },
                    arguments: "".to_string(),
                    autorerun_count: 0,
                    ignore_previous_failures: false,
                    requires_arguments: false,
                    arguments_hint: "\"arg1\" \"arg2\"".to_string(),
                };
                add_script_to_config(self, config::ScriptDefinition::Original(script));

                update_config_cache(&mut self.app_config, &self.edit_data);
            }
            WindowMessage::MoveExecutionScriptUp(script_idx) => {
                self.execution_data
                    .get_edited_execution_list_mut()
                    .swap(script_idx, script_idx - 1);
                select_execution_script(self, script_idx - 1);
            }
            WindowMessage::MoveExecutionScriptDown(script_idx) => {
                self.execution_data
                    .get_edited_execution_list_mut()
                    .swap(script_idx, script_idx + 1);
                select_execution_script(self, script_idx + 1);
            }
            WindowMessage::EditScriptName(new_name) => {
                if let Some(preset) =
                    get_editing_preset(&mut self.app_config, &self.edit_data, &self.window_state)
                {
                    preset.name = new_name;
                    self.edit_data.is_dirty = true;
                    update_config_cache(&mut self.app_config, &self.edit_data);
                } else {
                    apply_script_edit(self, move |script| script.name = new_name);
                }
            }
            WindowMessage::EditScriptCommand(new_command) => {
                apply_script_edit(self, move |script| script.command.path = new_command);
            }
            WindowMessage::ToggleScriptCommandRelativeToScripter(value) => {
                apply_script_edit(self, |script| {
                    script.command.path_type = if value {
                        config::PathType::ScripterExecutableRelative
                    } else {
                        config::PathType::WorkingDirRelative
                    }
                });
            }
            WindowMessage::EditScriptWorkingDirectory(new_working_directory) => {
                apply_script_edit(self, move |script| {
                    script.working_directory.path = new_working_directory
                });
            }
            WindowMessage::ToggleScriptWorkingDirectoryRelativeToScripter(value) => {
                apply_script_edit(self, |script| {
                    script.working_directory.path_type = if value {
                        config::PathType::ScripterExecutableRelative
                    } else {
                        config::PathType::WorkingDirRelative
                    }
                });
            }
            WindowMessage::EditScriptIconPath(new_icon_path) => {
                if let Some(preset) =
                    get_editing_preset(&mut self.app_config, &self.edit_data, &self.window_state)
                {
                    preset.icon.path = new_icon_path;
                    self.edit_data.is_dirty = true;
                    update_config_cache(&mut self.app_config, &self.edit_data);
                } else {
                    apply_script_edit(self, move |script| script.icon.path = new_icon_path);
                }
            }
            WindowMessage::ToggleScriptIconPathRelativeToScripter(new_relative) => {
                let new_path_type = if new_relative {
                    config::PathType::ScripterExecutableRelative
                } else {
                    config::PathType::WorkingDirRelative
                };

                if let Some(preset) =
                    get_editing_preset(&mut self.app_config, &self.edit_data, &self.window_state)
                {
                    preset.icon.path_type = new_path_type;
                    self.edit_data.is_dirty = true;
                    update_config_cache(&mut self.app_config, &self.edit_data);
                } else {
                    apply_script_edit(self, move |script| {
                        script.icon.path_type = new_path_type;
                    });
                }
            }
            WindowMessage::EditArguments(new_arguments) => {
                apply_script_edit(self, move |script| script.arguments = new_arguments)
            }
            WindowMessage::ToggleRequiresArguments(new_requires_arguments) => {
                apply_script_edit(self, move |script| {
                    script.requires_arguments = new_requires_arguments
                })
            }
            WindowMessage::EditArgumentsHint(new_arguments_hint) => {
                apply_script_edit(self, move |script| {
                    script.arguments_hint = new_arguments_hint
                })
            }
            WindowMessage::EditAutorerunCount(new_autorerun_count_str) => {
                let parse_result = usize::from_str(&new_autorerun_count_str);
                let mut new_autorerun_count = None;
                if let Ok(parse_result) = parse_result {
                    self.visual_caches.autorerun_count = new_autorerun_count_str;
                    new_autorerun_count = Some(parse_result);
                } else {
                    // if input is empty, then keep it empty and assume 0, otherwise keep the old value
                    if new_autorerun_count_str.is_empty() {
                        self.visual_caches.autorerun_count = new_autorerun_count_str;
                        new_autorerun_count = Some(0);
                    }
                }

                if let Some(new_autorerun_count) = new_autorerun_count {
                    apply_script_edit(self, |script| script.autorerun_count = new_autorerun_count)
                }
            }
            WindowMessage::ToggleIgnoreFailures(value) => {
                apply_script_edit(self, |script| script.ignore_previous_failures = value)
            }
            WindowMessage::EnterWindowEditMode => enter_window_edit_mode(self),
            WindowMessage::ExitWindowEditMode => exit_window_edit_mode(self),
            WindowMessage::TrySwitchWindowEditMode => {
                if !self.execution_data.has_started_execution() {
                    if !self.edit_data.window_edit_data.is_some() {
                        enter_window_edit_mode(self);
                    } else {
                        exit_window_edit_mode(self);
                    }
                }
            }
            WindowMessage::SaveConfig => {
                config::save_config_to_file(&self.app_config);
                self.app_config = config::read_config();
                self.edit_data.is_dirty = false;
                update_config_cache(&mut self.app_config, &self.edit_data);
            }
            WindowMessage::RevertConfig => {
                self.app_config = config::read_config();
                self.edit_data.window_edit_data = Some(WindowEditData::from_config(
                    &self.app_config,
                    false,
                    match self.edit_data.window_edit_data {
                        Some(WindowEditData {
                            edit_type: ConfigEditType::Local,
                            ..
                        }) => ConfigEditType::Local,
                        _ => ConfigEditType::Shared,
                    },
                ));
                config::populate_shared_scripts_from_config(&mut self.app_config);
                apply_theme(self);
                self.edit_data.is_dirty = false;
                clean_script_selection(&mut self.window_state.cursor_script);
                update_config_cache(&mut self.app_config, &self.edit_data);
            }
            WindowMessage::OpenScriptConfigEditing(script_idx) => {
                select_edited_script(self, script_idx);
            }
            WindowMessage::MoveConfigScriptUp(index) => {
                move_config_script_up(self, index);
            }
            WindowMessage::MoveConfigScriptDown(index) => {
                move_config_script_down(self, index);
            }
            WindowMessage::ToggleConfigEditing => {
                match &mut self.edit_data.window_edit_data {
                    Some(window_edit_data) => {
                        window_edit_data.is_editing_config = !window_edit_data.is_editing_config;
                    }
                    None => {
                        self.edit_data.window_edit_data = Some(WindowEditData::from_config(
                            &self.app_config,
                            true,
                            if self.app_config.local_config_body.is_some() {
                                ConfigEditType::Local
                            } else {
                                ConfigEditType::Shared
                            },
                        ));
                    }
                };
                clean_script_selection(&mut self.window_state.cursor_script);
            }
            WindowMessage::ConfigToggleAlwaysOnTop(is_checked) => {
                get_rewritable_config_mut(&mut self.app_config, &self.edit_data.window_edit_data)
                    .always_on_top = is_checked;
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigToggleWindowStatusReactions(is_checked) => {
                get_rewritable_config_mut(&mut self.app_config, &self.edit_data.window_edit_data)
                    .window_status_reactions = is_checked;
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigToggleKeepWindowSize(is_checked) => {
                get_rewritable_config_mut(&mut self.app_config, &self.edit_data.window_edit_data)
                    .keep_window_size = is_checked;
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigToggleScriptFiltering(is_checked) => {
                get_rewritable_config_mut(&mut self.app_config, &self.edit_data.window_edit_data)
                    .enable_script_filtering = is_checked;
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigToggleTitleEditing(is_checked) => {
                get_rewritable_config_mut(&mut self.app_config, &self.edit_data.window_edit_data)
                    .enable_title_editing = is_checked;
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigToggleUseCustomTheme(is_checked) => {
                get_rewritable_config_mut(&mut self.app_config, &self.edit_data.window_edit_data)
                    .custom_theme = if is_checked {
                    Some(
                        if let Some(window_edit_data) = &self.edit_data.window_edit_data {
                            config::CustomTheme {
                                background: color_utils::hex_to_rgb(
                                    &window_edit_data.theme_color_background,
                                )
                                .unwrap_or_default(),
                                text: color_utils::hex_to_rgb(&window_edit_data.theme_color_text)
                                    .unwrap_or_default(),
                                primary: color_utils::hex_to_rgb(
                                    &window_edit_data.theme_color_primary,
                                )
                                .unwrap_or_default(),
                                success: color_utils::hex_to_rgb(
                                    &window_edit_data.theme_color_success,
                                )
                                .unwrap_or_default(),
                                danger: color_utils::hex_to_rgb(
                                    &window_edit_data.theme_color_danger,
                                )
                                .unwrap_or_default(),
                                caption_text: color_utils::hex_to_rgb(
                                    &window_edit_data.theme_color_caption_text,
                                )
                                .unwrap_or_default(),
                                error_text: color_utils::hex_to_rgb(
                                    &window_edit_data.theme_color_error_text,
                                )
                                .unwrap_or_default(),
                            }
                        } else {
                            config::CustomTheme::default()
                        },
                    )
                } else {
                    None
                };
                apply_theme(self);
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigEditThemeBackground(new_value) => {
                apply_theme_color_from_string(
                    self,
                    new_value,
                    |theme, value| theme.background = value,
                    |edit_data, value| {
                        edit_data.theme_color_background = value;
                        edit_data.theme_color_background.clone()
                    },
                );
            }
            WindowMessage::ConfigEditThemeText(new_value) => {
                apply_theme_color_from_string(
                    self,
                    new_value,
                    |theme, value| theme.text = value,
                    |edit_data, value| {
                        edit_data.theme_color_text = value;
                        edit_data.theme_color_text.clone()
                    },
                );
            }
            WindowMessage::ConfigEditThemePrimary(new_value) => {
                apply_theme_color_from_string(
                    self,
                    new_value,
                    |theme, value| theme.primary = value,
                    |edit_data, value| {
                        edit_data.theme_color_primary = value;
                        edit_data.theme_color_primary.clone()
                    },
                );
            }
            WindowMessage::ConfigEditThemeSuccess(new_value) => {
                apply_theme_color_from_string(
                    self,
                    new_value,
                    |theme, value| theme.success = value,
                    |edit_data, value| {
                        edit_data.theme_color_success = value;
                        edit_data.theme_color_success.clone()
                    },
                );
            }
            WindowMessage::ConfigEditThemeDanger(new_value) => {
                apply_theme_color_from_string(
                    self,
                    new_value,
                    |theme, value| theme.danger = value,
                    |edit_data, value| {
                        edit_data.theme_color_danger = value;
                        edit_data.theme_color_danger.clone()
                    },
                );
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigEditThemeCaptionText(new_value) => {
                apply_theme_color_from_string(
                    self,
                    new_value,
                    |theme, value| theme.caption_text = value,
                    |edit_data, value| {
                        edit_data.theme_color_caption_text = value;
                        edit_data.theme_color_caption_text.clone()
                    },
                );
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigEditThemeErrorText(new_value) => {
                apply_theme_color_from_string(
                    self,
                    new_value,
                    |theme, value| theme.error_text = value,
                    |edit_data, value| {
                        edit_data.theme_color_error_text = value;
                        edit_data.theme_color_error_text.clone()
                    },
                );
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigEditLocalConfigPath(new_value) => {
                self.app_config.local_config_path.path = new_value;
                self.edit_data.is_dirty = true;
            }
            WindowMessage::ConfigToggleLocalConfigPathRelativeToScripter(is_checked) => {
                self.app_config.local_config_path.path_type = if is_checked {
                    config::PathType::ScripterExecutableRelative
                } else {
                    config::PathType::WorkingDirRelative
                };
                self.edit_data.is_dirty = true;
            }
            WindowMessage::SwitchToSharedConfig => {
                switch_to_editing_shared_config(self);
            }
            WindowMessage::SwitchToLocalConfig => {
                clean_script_selection(&mut self.window_state.cursor_script);
                switch_config_edit_mode(self, ConfigEditType::Local);
                apply_theme(self);
                update_config_cache(&mut self.app_config, &self.edit_data);
            }
            WindowMessage::ToggleScriptHidden(is_hidden) => {
                let Some(script_id) = &mut self.window_state.cursor_script else {
                    return Command::none();
                };

                if let Some(config) = &mut self.app_config.local_config_body {
                    let Some(script) = config.script_definitions.get_mut(script_id.idx) else {
                        return Command::none();
                    };

                    match script {
                        config::ScriptDefinition::ReferenceToShared(_, is_hidden_value) => {
                            *is_hidden_value = is_hidden;
                            self.edit_data.is_dirty = true;
                        }
                        _ => {}
                    }
                }
                update_config_cache(&mut self.app_config, &self.edit_data);
            }
            WindowMessage::CreateCopyOfSharedScript(script_id) => {
                let script = if let Some(config) = &self.app_config.local_config_body {
                    if let Some(script) = config.script_definitions.get(script_id.idx) {
                        script
                    } else {
                        return Command::none();
                    }
                } else {
                    return Command::none();
                };

                let new_script = match script {
                    config::ScriptDefinition::ReferenceToShared(shared_script_id, _is_hidden) => {
                        if let Some(mut script) = config::get_original_script_definition_by_uid(
                            &self.app_config,
                            shared_script_id.clone(),
                        ) {
                            match &mut script {
                                config::ScriptDefinition::Original(original_script) => {
                                    original_script.uid = config::Guid::new();
                                    original_script.name =
                                        format!("{} (copy)", original_script.name);
                                    script
                                }
                                _ => {
                                    return Command::none();
                                }
                            }
                        } else {
                            return Command::none();
                        }
                    }
                    _ => {
                        return Command::none();
                    }
                };

                if let Some(config) = &mut self.app_config.local_config_body {
                    config
                        .script_definitions
                        .insert(script_id.idx + 1, new_script);
                    select_edited_script(self, script_id.idx + 1);
                    self.edit_data.is_dirty = true;
                }
                update_config_cache(&mut self.app_config, &self.edit_data);
            }
            WindowMessage::MoveToShared(script_id) => {
                if let Some(config) = &mut self.app_config.local_config_body {
                    if config.script_definitions.len() <= script_id.idx {
                        return Command::none();
                    }

                    let insert_position = find_best_shared_script_insert_position(
                        &config.script_definitions,
                        &self.app_config.script_definitions,
                        &script_id,
                    );

                    if let Some(script) = config.script_definitions.get_mut(script_id.idx) {
                        let mut replacement_script = match script {
                            config::ScriptDefinition::Original(definition) => {
                                config::ScriptDefinition::ReferenceToShared(
                                    definition.uid.clone(),
                                    false,
                                )
                            }
                            config::ScriptDefinition::Preset(preset) => {
                                config::ScriptDefinition::ReferenceToShared(
                                    preset.uid.clone(),
                                    false,
                                )
                            }
                            _ => {
                                return Command::none();
                            }
                        };

                        swap(script, &mut replacement_script);
                        self.app_config
                            .script_definitions
                            .insert(insert_position, replacement_script);
                        self.edit_data.is_dirty = true;
                        switch_to_editing_shared_config(self);
                        select_edited_script(self, insert_position);
                    }
                }
            }
            WindowMessage::SaveAsPreset => {
                let mut preset = config::ScriptPreset {
                    uid: config::Guid::new(),
                    name: "new preset".to_string(),
                    icon: Default::default(),
                    items: vec![],
                };

                for script in self.execution_data.get_edited_execution_list() {
                    match script {
                        config::ScriptDefinition::Original(script) => {
                            let original_script = config::get_original_script_definition_by_uid(
                                &self.app_config,
                                script.uid.clone(),
                            );

                            let original_script = if let Some(original_script) = original_script {
                                match original_script {
                                    config::ScriptDefinition::ReferenceToShared(uid, _) => {
                                        config::get_original_script_definition_by_uid(
                                            &self.app_config,
                                            uid,
                                        )
                                    }
                                    _ => Some(original_script),
                                }
                            } else {
                                None
                            };

                            let original_script = if let Some(original_script) = original_script {
                                match original_script {
                                    config::ScriptDefinition::Original(script) => Some(script),
                                    _ => None,
                                }
                            } else {
                                None
                            };

                            let name = if let Some(original_script) = &original_script {
                                if original_script.name == script.name {
                                    None
                                } else {
                                    Some(script.name.clone())
                                }
                            } else {
                                Some(script.name.clone())
                            };

                            let arguments = if let Some(original_script) = &original_script {
                                if original_script.arguments == script.arguments {
                                    None
                                } else {
                                    Some(script.arguments.clone())
                                }
                            } else {
                                Some(script.arguments.clone())
                            };

                            let autorerun_count = if let Some(original_script) = &original_script {
                                if original_script.autorerun_count == script.autorerun_count {
                                    None
                                } else {
                                    Some(script.autorerun_count)
                                }
                            } else {
                                Some(script.autorerun_count)
                            };

                            let ignore_previous_failures =
                                if let Some(original_script) = original_script {
                                    if original_script.ignore_previous_failures
                                        == script.ignore_previous_failures
                                    {
                                        None
                                    } else {
                                        Some(script.ignore_previous_failures)
                                    }
                                } else {
                                    Some(script.ignore_previous_failures)
                                };

                            preset.items.push(config::PresetItem {
                                uid: script.uid.clone(),
                                name,
                                arguments,
                                autorerun_count,
                                ignore_previous_failures,
                            });
                        }
                        _ => {}
                    }
                }

                add_script_to_config(self, config::ScriptDefinition::Preset(preset));
            }
            WindowMessage::ScriptFilterChanged(new_filter_value) => {
                self.edit_data.script_filter = new_filter_value;
                update_config_cache(&mut self.app_config, &self.edit_data);
                clean_script_selection(&mut self.window_state.cursor_script);
            }
            WindowMessage::RequestCloseApp => {
                let exit_thread_command = || {
                    Command::perform(async {}, |()| {
                        std::process::exit(0);
                    })
                };

                if self.execution_data.has_started_execution() {
                    if self.execution_data.has_finished_execution() {
                        if !self.execution_data.is_waiting_execution_to_finish() {
                            return exit_thread_command();
                        }
                    }
                } else {
                    return exit_thread_command();
                }
            }
            WindowMessage::FocusFilter => {
                return focus_filter(self);
            }
            WindowMessage::OnCommandKeyStateChanged(is_command_key_down) => {
                self.window_state.is_command_key_down = is_command_key_down;
            }
            WindowMessage::MoveCursorUp => {
                move_cursor(self, true);
            }
            WindowMessage::MoveCursorDown => {
                move_cursor(self, false);
            }
            WindowMessage::MoveScriptDown => {
                if self.execution_data.has_started_execution() {
                    return Command::none();
                }

                let focused_pane = if let Some(focus) = self.window_state.pane_focus {
                    self.panes.panes[&focus].variant
                } else {
                    return Command::none();
                };

                if focused_pane == PaneVariant::ScriptList {
                    if self.edit_data.window_edit_data.is_some() {
                        if let Some(edited_script) = &self.window_state.cursor_script {
                            move_config_script_down(self, edited_script.idx);
                        }
                    }
                } else if focused_pane == PaneVariant::ExecutionList {
                    if let Some(cursor_script) = &self.window_state.cursor_script {
                        if cursor_script.script_type == EditScriptType::ExecutionList {
                            if cursor_script.idx + 1
                                >= self.execution_data.get_edited_execution_list().len()
                            {
                                return Command::none();
                            }
                            self.execution_data
                                .get_edited_execution_list_mut()
                                .swap(cursor_script.idx, cursor_script.idx + 1);
                            select_execution_script(self, cursor_script.idx + 1);
                        }
                    }
                }
            }
            WindowMessage::MoveScriptUp => {
                let focused_pane = if let Some(focus) = self.window_state.pane_focus {
                    self.panes.panes[&focus].variant
                } else {
                    return Command::none();
                };

                if focused_pane == PaneVariant::ScriptList {
                    if self.edit_data.window_edit_data.is_some() {
                        if let Some(edited_script) = &self.window_state.cursor_script {
                            move_config_script_up(self, edited_script.idx);
                        }
                    }
                } else if focused_pane == PaneVariant::ExecutionList {
                    if let Some(cursor_script) = &self.window_state.cursor_script {
                        if cursor_script.script_type == EditScriptType::ExecutionList {
                            if cursor_script.idx == 0 {
                                return Command::none();
                            }
                            self.execution_data
                                .get_edited_execution_list_mut()
                                .swap(cursor_script.idx, cursor_script.idx - 1);
                            select_execution_script(self, cursor_script.idx - 1);
                        }
                    }
                }
            }
            WindowMessage::CursorConfirm => {
                if self.edit_data.window_edit_data.is_some() {
                    return Command::none();
                }

                let Some(cursor_script) = &self.window_state.cursor_script else {
                    return Command::none();
                };

                let cursor_script_id = cursor_script.idx;

                if let Some(focus) = self.window_state.pane_focus {
                    if &self.panes.panes[&focus].variant == &PaneVariant::ScriptList {
                        let scripts = &self.app_config.displayed_configs_list_cache;

                        if let Some(script) = scripts.get(cursor_script_id) {
                            let is_added = add_script_to_execution(
                                self,
                                script.original_script_uid.clone(),
                                false,
                            );

                            if is_added && self.window_state.is_command_key_down {
                                run_scheduled_scripts(self);
                            }
                        }
                    }
                }
            }
            WindowMessage::RemoveCursorScript => {
                if self.execution_data.has_started_execution() {
                    return Command::none();
                }

                if let Some(focus) = self.window_state.pane_focus {
                    if &self.panes.panes[&focus].variant != &PaneVariant::ExecutionList {
                        return Command::none();
                    }
                }

                if let Some(cursor_script) = self.window_state.cursor_script.clone() {
                    if cursor_script.script_type == EditScriptType::ExecutionList {
                        remove_script(self, &cursor_script);
                    }
                }
            }
            WindowMessage::SwitchPaneFocus(is_forward) => {
                let new_selection = get_next_pane_selection(self, is_forward);

                let mut should_select_arguments = false;
                let has_pane_changed = Some(new_selection)
                    != if let Some(focus) = self.window_state.pane_focus {
                        Some(self.panes.panes[&focus].variant)
                    } else {
                        None
                    };

                if new_selection == PaneVariant::Parameters {
                    if let Some(focus) = self.window_state.pane_focus {
                        if self.panes.panes[&focus].variant != PaneVariant::Parameters {
                            if let Some(cursor_script) = &self.window_state.cursor_script {
                                match cursor_script.script_type {
                                    EditScriptType::ScriptConfig => {
                                        should_select_arguments =
                                            self.edit_data.window_edit_data.is_some();
                                    }
                                    EditScriptType::ExecutionList => {
                                        should_select_arguments = true;
                                    }
                                }
                            }
                        }
                    }
                }

                self.window_state.pane_focus = Some(self.pane_by_pane_type[&new_selection]);

                if should_select_arguments {
                    return text_input::focus(ARGUMENTS_INPUT_ID.clone());
                } else if has_pane_changed {
                    return text_input::focus(text_input::Id::new("dummy"));
                }
            }
            WindowMessage::SetExecutionListTitleEditing(is_editing) => {
                self.visual_caches.is_custom_title_editing = is_editing;
            }
            WindowMessage::EditExecutionListTitle(new_title) => {
                self.app_config.custom_title = Some(new_title);
            }
            WindowMessage::OpenWithDefaultApplication(file_path) => {
                if let Err(e) = open::that(file_path) {
                    eprintln!("Failed to open file with default application: {}", e);
                }
            }
            WindowMessage::OpenUrl(url) => {
                if let Err(e) = open::that(url) {
                    eprintln!("Failed to open URL: {}", e);
                }
            }
            WindowMessage::SwitchToOriginalSharedScript(local_script_id) => {
                let original_script_uid = {
                    let script = get_script_definition(
                        &self.app_config,
                        &self.edit_data,
                        local_script_id.idx,
                    );
                    match script {
                        config::ScriptDefinition::ReferenceToShared(uid, _) => uid,
                        _ => return Command::none(),
                    }
                };

                let mut original_script_idx = self.app_config.script_definitions.len();
                for (idx, script_definition) in
                    self.app_config.script_definitions.iter().enumerate()
                {
                    match script_definition {
                        config::ScriptDefinition::Original(script) => {
                            if script.uid == *original_script_uid {
                                original_script_idx = idx;
                                break;
                            }
                        }
                        config::ScriptDefinition::Preset(preset) => {
                            if preset.uid == *original_script_uid {
                                original_script_idx = idx;
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if original_script_idx == self.app_config.script_definitions.len() {
                    return Command::none();
                }

                switch_config_edit_mode(self, ConfigEditType::Shared);
                select_edited_script(self, original_script_idx);

                update_config_cache(&mut self.app_config, &self.edit_data);
            }
        }

        Command::none()
    }

    fn view(&self) -> Element<WindowMessage> {
        let focus = self.window_state.pane_focus;
        let total_panes = self.panes.len();

        let pane_grid = responsive(move |size| {
            PaneGrid::new(&self.panes, |id, pane, is_maximized| {
                let is_focused = focus == Some(id);

                let variant = &pane.variant;

                let title = row![get_pane_name_from_variant(variant)].spacing(5);

                let title_bar = pane_grid::TitleBar::new(title)
                    .controls(view_controls(
                        id,
                        variant,
                        total_panes,
                        &self.visual_caches.icons,
                        &self.edit_data,
                        &self.execution_data,
                        is_maximized,
                        size,
                        &self.window_state,
                    ))
                    .padding(10)
                    .style(if is_focused {
                        if self.execution_data.has_failed_scripts() {
                            style::title_bar_focused_failed
                        } else if self.execution_data.has_finished_execution() {
                            style::title_bar_focused_completed
                        } else {
                            style::title_bar_focused
                        }
                    } else {
                        style::title_bar_active
                    });

                pane_grid::Content::new(responsive(move |_size| {
                    view_content(
                        &self.execution_data,
                        variant,
                        &self.theme,
                        &self.app_config.paths,
                        &self.visual_caches,
                        &self.app_config,
                        &self.edit_data,
                        &self.window_state,
                    )
                }))
                .title_bar(title_bar)
                .style(if is_focused {
                    style::pane_focused
                } else {
                    style::pane_active
                })
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(1)
            .on_click(WindowMessage::Clicked)
            .on_drag(WindowMessage::Dragged)
            .on_resize(10, WindowMessage::Resized)
            .into()
        });

        container(pane_grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(1)
            .into()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<WindowMessage> {
        use keyboard::KeyCode;

        Subscription::batch([
            iced::subscription::events_with(|event, status| {
                let is_command_key = |key_code: KeyCode| {
                    #[cfg(target_os = "macos")]
                    {
                        key_code.eq(&KeyCode::LWin) || key_code.eq(&KeyCode::RWin)
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        key_code.eq(&KeyCode::LControl) || key_code.eq(&KeyCode::RControl)
                    }
                };

                match event {
                    Event::Window(window::Event::Resized { width, height }) => {
                        Some(WindowMessage::WindowResized(Size {
                            width: width as f32,
                            height: height as f32,
                        }))
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        modifiers,
                        key_code,
                    }) => {
                        if is_command_key(key_code) {
                            return Some(WindowMessage::OnCommandKeyStateChanged(true));
                        }

                        let is_input_captured_by_a_widget = if let event::Status::Captured = status
                        {
                            true
                        } else {
                            false
                        };

                        let is_command_key_down = modifiers.command();
                        let is_shift_key_down = modifiers.shift();
                        if is_command_key_down {
                            handle_command_hotkey(
                                key_code,
                                &status,
                                is_shift_key_down,
                                is_input_captured_by_a_widget,
                            )
                        } else if is_shift_key_down {
                            handle_shift_hotkey(key_code, &status, is_input_captured_by_a_widget)
                        } else {
                            handle_key_press(key_code, &status, is_input_captured_by_a_widget)
                        }
                    }
                    Event::Keyboard(keyboard::Event::KeyReleased {
                        modifiers: _modifiers,
                        key_code,
                    }) => {
                        if is_command_key(key_code) {
                            Some(WindowMessage::OnCommandKeyStateChanged(false))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }),
            time::every(Duration::from_millis(100)).map(WindowMessage::Tick),
        ])
    }
}

fn handle_command_hotkey(
    key_code: keyboard::KeyCode,
    _status: &event::Status,
    is_shift_key_down: bool,
    is_input_captured_by_a_widget: bool,
) -> Option<WindowMessage> {
    use keyboard::KeyCode;

    match key_code {
        KeyCode::W => Some(WindowMessage::RequestCloseApp),
        KeyCode::F => Some(WindowMessage::FocusFilter),
        KeyCode::E => Some(WindowMessage::TrySwitchWindowEditMode),
        KeyCode::R => {
            if is_shift_key_down {
                Some(WindowMessage::RescheduleScripts)
            } else {
                Some(WindowMessage::RunScripts)
            }
        }
        KeyCode::C => {
            if !is_input_captured_by_a_widget {
                if is_shift_key_down {
                    Some(WindowMessage::StopScripts)
                } else {
                    Some(WindowMessage::ClearExecutionScripts)
                }
            } else {
                None
            }
        }
        KeyCode::Q => Some(WindowMessage::MaximizeOrRestoreExecutionPane),
        KeyCode::Enter => Some(WindowMessage::CursorConfirm),
        _ => None,
    }
}

fn handle_shift_hotkey(
    key_code: keyboard::KeyCode,
    _status: &event::Status,
    _is_input_captured_by_a_widget: bool,
) -> Option<WindowMessage> {
    use keyboard::KeyCode;

    match key_code {
        KeyCode::Down => Some(WindowMessage::MoveScriptDown),
        KeyCode::Up => Some(WindowMessage::MoveScriptUp),
        KeyCode::Tab => Some(WindowMessage::SwitchPaneFocus(false)),
        _ => None,
    }
}

fn handle_key_press(
    key_code: keyboard::KeyCode,
    _status: &event::Status,
    _is_input_captured_by_a_widget: bool,
) -> Option<WindowMessage> {
    use keyboard::KeyCode;

    match key_code {
        KeyCode::Down => Some(WindowMessage::MoveCursorDown),
        KeyCode::Up => Some(WindowMessage::MoveCursorUp),
        KeyCode::Enter => Some(WindowMessage::CursorConfirm),
        KeyCode::Tab => Some(WindowMessage::SwitchPaneFocus(true)),
        KeyCode::Delete => Some(WindowMessage::RemoveCursorScript),
        _ => None,
    }
}

fn set_selected_script(
    currently_edited_script: &mut Option<EditScriptId>,
    scripts_to_run: &Vec<config::ScriptDefinition>,
    script_definitions: &Vec<config::ScriptDefinition>,
    visual_caches: &mut VisualCaches,
    script_idx: usize,
    script_type: EditScriptType,
) {
    *currently_edited_script = Some(EditScriptId {
        idx: script_idx,
        script_type: script_type.clone(),
    });

    // get autorerun count text from value
    match &script_type {
        EditScriptType::ScriptConfig => {
            if let Some(script) = &script_definitions.get(script_idx) {
                match script {
                    config::ScriptDefinition::Original(script) => {
                        visual_caches.autorerun_count = script.autorerun_count.to_string();
                    }
                    _ => {}
                }
            }
        }
        EditScriptType::ExecutionList => {
            if let Some(config::ScriptDefinition::Original(script)) =
                &scripts_to_run.get(script_idx)
            {
                visual_caches.autorerun_count = script.autorerun_count.to_string();
            }
        }
    };
}

fn clean_script_selection(currently_edited_script: &mut Option<EditScriptId>) {
    *currently_edited_script = None;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneVariant {
    ScriptList,
    ExecutionList,
    LogOutput,
    Parameters,
}

struct AppPane {
    variant: PaneVariant,
}

impl AppPane {
    fn new(variant: PaneVariant) -> Self {
        Self { variant }
    }
}

fn inline_icon_button<'a, Message>(icon_handle: Handle, message: Message) -> Button<'a, Message> {
    button(
        image(icon_handle)
            .width(Length::Fixed(14.0))
            .height(Length::Fixed(14.0)),
    )
    .padding(4)
    .on_press(message)
}

fn main_icon_button(
    icon_handle: Handle,
    label: &str,
    message: Option<WindowMessage>,
) -> Button<WindowMessage> {
    let new_button = button(row![
        image(icon_handle)
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(16.0)),
        horizontal_space(4),
        text(label).width(Length::Shrink).size(16),
    ])
    .width(Length::Shrink)
    .padding(8);

    if let Some(message) = message {
        new_button.on_press(message)
    } else {
        new_button
    }
}

fn main_button(label: &str, message: WindowMessage) -> Button<WindowMessage> {
    button(row![text(label).width(Length::Shrink).size(16),])
        .width(Length::Shrink)
        .padding(8)
        .on_press(message)
}

fn edit_mode_button<'a>(
    icon_handle: Handle,
    message: WindowMessage,
    is_dirty: bool,
    window_state: &WindowState,
) -> Button<'a, WindowMessage> {
    let icon = image(icon_handle)
        .width(Length::Fixed(12.0))
        .height(Length::Fixed(12.0));

    button(if window_state.is_command_key_down {
        row![
            text(string_constants::EDIT_COMMAND_HINT).size(12),
            horizontal_space(4),
            icon
        ]
    } else {
        row![icon]
    })
    .style(if is_dirty {
        theme::Button::Positive
    } else {
        theme::Button::Secondary
    })
    .width(Length::Shrink)
    .padding(4)
    .on_press(message)
}

fn produce_script_list_content<'a>(
    config: &config::AppConfig,
    rewritable_config: &config::RewritableConfig,
    edit_data: &EditData,
    icons: &ui_icons::IconCaches,
    window_state: &WindowState,
    theme: &Theme,
) -> Column<'a, WindowMessage> {
    if let Some(error) = &config.config_read_error {
        return get_config_error_content(error, theme);
    }

    let data: Element<_> = column(
        config
            .displayed_configs_list_cache
            .iter()
            .enumerate()
            .map(|(i, script)| {
                let mut name_text = script.name.clone();

                if is_local_edited_script(i, &config, &edit_data.window_edit_data) {
                    name_text += " [local]";
                }
                if script.is_hidden {
                    name_text += " [hidden]";
                }

                let will_run_on_click =
                    edit_data.window_edit_data.is_none() && window_state.is_command_key_down;

                let edit_buttons = if edit_data.window_edit_data.is_some() {
                    row![
                        inline_icon_button(
                            icons.themed.up.clone(),
                            WindowMessage::MoveConfigScriptUp(i)
                        ),
                        horizontal_space(5),
                        inline_icon_button(
                            icons.themed.down.clone(),
                            WindowMessage::MoveConfigScriptDown(i)
                        ),
                        horizontal_space(5),
                    ]
                } else {
                    row![]
                };

                let icon = if will_run_on_click {
                    row![
                        horizontal_space(6),
                        image(icons.themed.quick_launch.clone())
                            .width(22)
                            .height(22),
                    ]
                } else if let Some(icon_path) = &script.full_icon_path {
                    row![horizontal_space(6), image(icon_path).width(22).height(22),]
                } else {
                    row![]
                };

                let is_selected = match &window_state.cursor_script {
                    Some(EditScriptId { idx, script_type })
                        if *idx == i && *script_type == EditScriptType::ScriptConfig =>
                    {
                        true
                    }
                    _ => false,
                };

                let item_button = button(
                    row![
                        icon,
                        horizontal_space(6),
                        text(&name_text).height(22),
                        horizontal_space(Length::Fill),
                        edit_buttons,
                    ]
                    .height(22),
                )
                .padding(4)
                .style(if is_selected {
                    theme::Button::Primary
                } else {
                    theme::Button::Secondary
                })
                .on_press(if edit_data.window_edit_data.is_none() {
                    WindowMessage::AddScriptToExecution(script.original_script_uid.clone())
                } else {
                    WindowMessage::OpenScriptConfigEditing(i)
                });

                row![item_button].into()
            })
            .collect(),
    )
    .width(Length::Fill)
    .into();

    let data_column = if let Some(window_edit_data) = &edit_data.window_edit_data {
        column![
            data,
            vertical_space(Length::Fixed(4.0)),
            row![
                main_icon_button(
                    icons.themed.plus.clone(),
                    "Add script",
                    Some(WindowMessage::AddScriptToConfig)
                ),
                horizontal_space(Length::Fixed(4.0)),
                main_icon_button(
                    icons.themed.settings.clone(),
                    "Settings",
                    Some(WindowMessage::ToggleConfigEditing)
                ),
            ],
            if config.local_config_body.is_some() {
                match window_edit_data.edit_type {
                    ConfigEditType::Local => {
                        column![
                            vertical_space(Length::Fixed(4.0)),
                            button(text("Edit shared config").size(16))
                                .on_press(WindowMessage::SwitchToSharedConfig)
                        ]
                    }
                    ConfigEditType::Shared => {
                        column![
                            vertical_space(Length::Fixed(4.0)),
                            button(text("Edit local config").size(16))
                                .on_press(WindowMessage::SwitchToLocalConfig)
                        ]
                    }
                }
            } else {
                column![]
            },
            if edit_data.is_dirty {
                column![
                    vertical_space(Length::Fixed(4.0)),
                    row![
                        main_icon_button(
                            icons.themed.back.clone(),
                            "Exit editing mode",
                            Some(WindowMessage::ExitWindowEditMode)
                        ),
                        horizontal_space(Length::Fixed(4.0)),
                        button(text("Save").size(16))
                            .style(theme::Button::Positive)
                            .on_press(WindowMessage::SaveConfig),
                        horizontal_space(Length::Fixed(4.0)),
                        button(text("Revert").size(16))
                            .style(theme::Button::Destructive)
                            .on_press(WindowMessage::RevertConfig),
                    ]
                ]
            } else {
                column![
                    vertical_space(Length::Fixed(4.0)),
                    main_icon_button(
                        icons.themed.back.clone(),
                        "Exit editing mode",
                        Some(WindowMessage::ExitWindowEditMode)
                    ),
                ]
            }
        ]
    } else {
        column![data]
    };

    let filter_field =
        if rewritable_config.enable_script_filtering && edit_data.window_edit_data.is_none() {
            row![
                horizontal_space(5),
                text_input(
                    if window_state.is_command_key_down {
                        string_constants::FILTER_COMMAND_HINT
                    } else {
                        "filter"
                    },
                    &edit_data.script_filter
                )
                .id(FILTER_INPUT_ID.clone())
                .on_input(WindowMessage::ScriptFilterChanged)
                .width(Length::Fill),
                horizontal_space(4),
                if !edit_data.script_filter.is_empty() {
                    column![
                        vertical_space(Length::Fixed(4.0)),
                        button(image(
                            (if theme.extended_palette().danger.base.text.r > 0.5 {
                                &icons.bright
                            } else {
                                &icons.dark
                            })
                            .remove
                            .clone()
                        ))
                        .style(theme::Button::Destructive)
                        .height(Length::Fixed(22.0))
                        .on_press(WindowMessage::ScriptFilterChanged("".to_string())),
                    ]
                } else {
                    column![]
                },
                horizontal_space(1),
            ]
        } else {
            row![]
        };

    column![filter_field, scrollable(data_column),]
        .width(Length::Fill)
        .height(Length::Fill)
        .align_items(Alignment::Start)
}

fn produce_execution_list_content<'a>(
    execution_lists: &execution_lists::ExecutionLists,
    path_caches: &config::PathCaches,
    theme: &Theme,
    custom_title: &Option<String>,
    visual_caches: &VisualCaches,
    edit_data: &EditData,
    rewritable_config: &config::RewritableConfig,
    window_state: &WindowState,
) -> Column<'a, WindowMessage> {
    let custom_title: String = if let Some(custom_title) = custom_title {
        custom_title.to_string()
    } else {
        "".to_string()
    };

    let icons = &visual_caches.icons;

    let title_widget = if visual_caches.is_custom_title_editing {
        row![
            text_input("Write a note for this execution here", &custom_title)
                .on_input(WindowMessage::EditExecutionListTitle)
                .on_submit(WindowMessage::SetExecutionListTitleEditing(false))
                .size(16)
                .width(Length::Fill),
        ]
    } else if rewritable_config.enable_title_editing && edit_data.window_edit_data.is_none() {
        row![
            horizontal_space(iced::Length::Fill),
            text(custom_title)
                .size(16)
                .horizontal_alignment(alignment::Horizontal::Center)
                .width(Length::Shrink),
            tooltip(
                button(
                    image(icons.themed.edit.clone())
                        .width(Length::Fixed(8.0))
                        .height(Length::Fixed(8.0))
                )
                .style(theme::Button::Secondary)
                .on_press(WindowMessage::SetExecutionListTitleEditing(true)),
                "Edit title",
                tooltip::Position::Right
            ),
            horizontal_space(iced::Length::Fill),
        ]
        .align_items(Alignment::Center)
    } else {
        row![text(custom_title)
            .size(16)
            .horizontal_alignment(alignment::Horizontal::Center)
            .width(Length::Fill),]
        .align_items(Alignment::Center)
    };

    let title = column![
        text(path_caches.work_path.to_str().unwrap_or_default())
            .size(16)
            .horizontal_alignment(alignment::Horizontal::Center)
            .width(Length::Fill),
        title_widget,
    ];

    let scheduled_data: Element<_> = column(
        execution_lists
            .get_scheduled_execution_list()
            .iter()
            .zip(execution_lists.get_scheduled_execution_statuses().iter())
            .enumerate()
            .map(|(i, (script, script_status))| {
                let config::ScriptDefinition::Original(script) = script else {
                    panic!("execution list definition is not Original");
                };
                let script_name = &script.name;

                let repeat_text = if script_status.retry_count > 0 {
                    format!(
                        " [{}/{}]",
                        script_status.retry_count, script.autorerun_count
                    )
                } else {
                    String::new()
                };

                let status;
                let status_tooltip;
                let progress;
                let style = if execution::has_script_failed(script_status) {
                    if let Some(custom_theme) = &rewritable_config.custom_theme {
                        iced::Color::from_rgb(
                            custom_theme.error_text[0],
                            custom_theme.error_text[1],
                            custom_theme.error_text[2],
                        )
                    } else {
                        theme.extended_palette().danger.weak.color
                    }
                } else {
                    theme.extended_palette().background.strong.text
                };

                if execution::has_script_finished(script_status) {
                    status = match script_status.result {
                        execution::ScriptResultStatus::Failed => image(icons.failed.clone()),
                        execution::ScriptResultStatus::Success => image(icons.succeeded.clone()),
                        execution::ScriptResultStatus::Skipped => image(icons.skipped.clone()),
                    };
                    status_tooltip = match script_status.result {
                        execution::ScriptResultStatus::Failed => "Failed",
                        execution::ScriptResultStatus::Success => "Success",
                        execution::ScriptResultStatus::Skipped => "Skipped",
                    };
                    if script_status.result != execution::ScriptResultStatus::Skipped {
                        let time_taken_sec = script_status
                            .finish_time
                            .unwrap_or(Instant::now())
                            .duration_since(script_status.start_time.unwrap_or(Instant::now()))
                            .as_secs();
                        progress = text(format!(
                            " ({:02}:{:02}){}",
                            time_taken_sec / 60,
                            time_taken_sec % 60,
                            repeat_text,
                        ))
                        .style(style);
                    } else {
                        progress = text("").style(style);
                    }
                } else if execution::has_script_started(script_status) {
                    let time_taken_sec = Instant::now()
                        .duration_since(script_status.start_time.unwrap_or(Instant::now()))
                        .as_secs();
                    status = image(icons.in_progress.clone());
                    status_tooltip = "In progress";

                    progress = text(format!(
                        " ({:02}:{:02}){}",
                        time_taken_sec / 60,
                        time_taken_sec % 60,
                        repeat_text,
                    ))
                    .style(style);
                } else {
                    status = image(icons.idle.clone());
                    status_tooltip = "Idle";
                    progress = text("").style(style);
                };

                let mut row_data: Vec<Element<'_, WindowMessage, iced::Renderer>> = Vec::new();
                row_data.push(
                    tooltip(
                        status.width(22).height(22).content_fit(ContentFit::None),
                        status_tooltip,
                        tooltip::Position::Right,
                    )
                    .style(theme::Container::Box)
                    .into(),
                );
                row_data.push(horizontal_space(4).into());
                if !script.icon.path.is_empty() {
                    row_data.push(
                        image(config::get_full_path(path_caches, &script.icon))
                            .width(22)
                            .height(22)
                            .into(),
                    );
                    row_data.push(horizontal_space(4).into());
                }
                row_data.push(text(script_name).style(style).into());
                row_data.push(progress.into());

                if execution::has_script_started(&script_status) {
                    row_data.push(horizontal_space(8).into());
                    if script_status.retry_count > 0 {
                        let log_dir_path = execution_lists.get_log_path();
                        row_data.push(
                            tooltip(
                                inline_icon_button(
                                    icons.themed.log.clone(),
                                    WindowMessage::OpenWithDefaultApplication(log_dir_path),
                                ),
                                "Open log directory",
                                tooltip::Position::Right,
                            )
                            .style(theme::Container::Box)
                            .into(),
                        );
                    } else if !execution::has_script_been_skipped(&script_status) {
                        let output_path = file_utils::get_script_output_path(
                            execution_lists.get_log_path().clone(),
                            script_name,
                            i as isize,
                            script_status.retry_count,
                        );
                        row_data.push(
                            tooltip(
                                inline_icon_button(
                                    icons.themed.log.clone(),
                                    WindowMessage::OpenWithDefaultApplication(output_path),
                                ),
                                "Open log file",
                                tooltip::Position::Right,
                            )
                            .style(theme::Container::Box)
                            .into(),
                        );
                    }
                }
                row(row_data).height(30).into()
            })
            .collect(),
    )
    .width(Length::Fill)
    .align_items(Alignment::Start)
    .into();

    let edited_data: Element<_> = column(
        execution_lists
            .get_edited_execution_list()
            .iter()
            .enumerate()
            .map(|(i, script)| {
                let config::ScriptDefinition::Original(script) = script else {
                    panic!("execution list definition is not Original");
                };
                let script_name = &script.name;

                let is_selected = match &window_state.cursor_script {
                    Some(selected_script) => {
                        selected_script.idx == i
                            && selected_script.script_type == EditScriptType::ExecutionList
                    }
                    None => false,
                };

                let style = if is_selected {
                    theme.extended_palette().primary.strong.text
                } else {
                    theme.extended_palette().background.strong.text
                };

                let mut row_data: Vec<Element<'_, WindowMessage, iced::Renderer>> = Vec::new();

                row_data.push(horizontal_space(4).into());
                if !script.icon.path.is_empty() {
                    row_data.push(
                        image(config::get_full_path(path_caches, &script.icon))
                            .width(22)
                            .height(22)
                            .into(),
                    );
                    row_data.push(horizontal_space(4).into());
                }
                row_data.push(text(script_name).style(style).into());

                if is_selected {
                    row_data.push(horizontal_space(Length::Fill).into());
                    if i > 0 {
                        row_data.push(
                            inline_icon_button(
                                icons.themed.up.clone(),
                                WindowMessage::MoveExecutionScriptUp(i),
                            )
                            .style(theme::Button::Primary)
                            .into(),
                        );
                    }
                    if i + 1 < execution_lists.get_edited_execution_list().len() {
                        row_data.push(
                            inline_icon_button(
                                icons.themed.down.clone(),
                                WindowMessage::MoveExecutionScriptDown(i),
                            )
                            .style(theme::Button::Primary)
                            .into(),
                        );
                    } else {
                        row_data.push(horizontal_space(22).into());
                    }
                    row_data.push(horizontal_space(8).into());
                    row_data.push(
                        tooltip(
                            inline_icon_button(
                                (if theme.extended_palette().danger.base.text.r > 0.5 {
                                    &icons.bright
                                } else {
                                    &icons.dark
                                })
                                .remove
                                .clone(),
                                WindowMessage::RemoveScript(EditScriptId {
                                    idx: i,
                                    script_type: EditScriptType::ExecutionList,
                                }),
                            )
                            .style(theme::Button::Destructive),
                            "Remove script from execution list",
                            tooltip::Position::Left,
                        )
                        .style(theme::Container::Box)
                        .into(),
                    );
                }

                let mut list_item = button(row(row_data)).width(Length::Fill).padding(4);
                if is_selected {
                    list_item = list_item.on_press(WindowMessage::CloseScriptEditing);
                } else {
                    list_item = list_item.on_press(WindowMessage::OpenScriptEditing(i));
                }

                list_item = list_item.style(if is_selected {
                    theme::Button::Primary
                } else {
                    if is_original_script_missing_arguments(&script) {
                        theme::Button::Destructive
                    } else {
                        theme::Button::Secondary
                    }
                });

                list_item.height(30).into()
            })
            .collect(),
    )
    .width(Length::Fill)
    .align_items(Alignment::Start)
    .into();

    let clear_name = if window_state.is_command_key_down {
        string_constants::CLEAR_COMMAND_HINT
    } else {
        "Clear"
    };

    let execution_controls = column![if execution_lists.has_finished_execution() {
        if !execution_lists.is_waiting_execution_to_finish() {
            row![
                main_icon_button(
                    icons.themed.retry.clone(),
                    if window_state.is_command_key_down {
                        string_constants::RESCHEDULE_COMMAND_HINT
                    } else {
                        "Reschedule"
                    },
                    Some(WindowMessage::RescheduleScripts)
                ),
                main_icon_button(
                    icons.themed.remove.clone(),
                    clear_name,
                    Some(WindowMessage::ClearExecutionScripts)
                ),
            ]
            .align_items(Alignment::Center)
            .spacing(5)
        } else {
            row![text("Waiting for the execution to stop")].align_items(Alignment::Center)
        }
    } else if execution_lists.has_started_execution() {
        let current_script = execution_lists.get_currently_outputting_script();
        if current_script != -1
            && execution::has_script_failed(
                &execution_lists.get_scheduled_execution_statuses()[current_script as usize],
            )
        {
            row![text("Waiting for the execution to stop")].align_items(Alignment::Center)
        } else {
            row![main_icon_button(
                icons.themed.stop.clone(),
                if window_state.is_command_key_down {
                    string_constants::STOP_COMMAND_HINT
                } else {
                    "Stop"
                },
                Some(WindowMessage::StopScripts)
            )]
            .align_items(Alignment::Center)
        }
    } else {
        row![].into()
    }]
    .spacing(5)
    .width(Length::Fill)
    .align_items(Alignment::Center);

    let edit_controls = column![if edit_data.window_edit_data.is_some() {
        if !execution_lists.get_edited_execution_list().is_empty() {
            row![main_button("Save as preset", WindowMessage::SaveAsPreset)]
                .align_items(Alignment::Center)
                .spacing(5)
        } else {
            row![]
        }
    } else if !execution_lists.get_edited_execution_list().is_empty() {
        let has_scripts_missing_arguments = execution_lists
            .get_edited_execution_list()
            .iter()
            .any(|script| is_script_missing_arguments(script));

        let run_name = if window_state.is_command_key_down {
            string_constants::RUN_COMMAND_HINT
        } else {
            "Run"
        };

        let run_button = if has_scripts_missing_arguments {
            column![tooltip(
                main_icon_button(icons.themed.play.clone(), run_name, None,),
                "Some scripts are missing arguments",
                tooltip::Position::Top
            )
            .style(theme::Container::Box)]
        } else {
            column![main_icon_button(
                icons.themed.play.clone(),
                run_name,
                Some(WindowMessage::RunScripts)
            ),]
        };
        row![
            run_button,
            main_icon_button(
                icons.themed.remove.clone(),
                clear_name,
                Some(WindowMessage::ClearExecutionScripts)
            ),
        ]
        .align_items(Alignment::Center)
        .spacing(5)
    } else {
        row![].into()
    }]
    .spacing(5)
    .width(Length::Fill)
    .align_items(Alignment::Center);

    let scheduled_block = column![
        scheduled_data,
        vertical_space(8),
        execution_controls,
        vertical_space(8),
    ];

    let edited_block = column![
        edited_data,
        vertical_space(8),
        edit_controls,
        vertical_space(8),
    ];

    return column![
        title,
        scrollable(column![
            if !execution_lists.get_scheduled_execution_list().is_empty() {
                scheduled_block
            } else {
                column![]
            },
            if !execution_lists.get_edited_execution_list().is_empty() {
                edited_block
            } else {
                column![]
            },
        ])
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .spacing(10)
    .align_items(Alignment::Center);
}

fn produce_log_output_content<'a>(
    execution_lists: &execution_lists::ExecutionLists,
    theme: &Theme,
    rewritable_config: &config::RewritableConfig,
) -> Column<'a, WindowMessage> {
    if !execution_lists.has_started_execution() {
        return Column::new();
    }

    let mut data_lines: Vec<Element<'_, WindowMessage, iced::Renderer>> = Vec::new();
    if let Ok(logs) = execution_lists.get_recent_logs().try_lock() {
        if !logs.is_empty() {
            let (caption_color, error_color) =
                if let Some(custom_theme) = &rewritable_config.custom_theme {
                    (
                        iced::Color::from_rgb(
                            custom_theme.caption_text[0],
                            custom_theme.caption_text[1],
                            custom_theme.caption_text[2],
                        ),
                        iced::Color::from_rgb(
                            custom_theme.error_text[0],
                            custom_theme.error_text[1],
                            custom_theme.error_text[2],
                        ),
                    )
                } else {
                    (
                        theme.extended_palette().primary.strong.color,
                        theme.extended_palette().danger.weak.color,
                    )
                };

            data_lines.extend(logs.iter().map(|element| {
                text(format!(
                    "[{}] {}",
                    element.timestamp.format("%H:%M:%S"),
                    element.text
                ))
                .style(match element.output_type {
                    execution::OutputType::StdOut => theme.extended_palette().primary.weak.text,
                    execution::OutputType::StdErr => error_color,
                    execution::OutputType::Error => error_color,
                    execution::OutputType::Event => caption_color,
                })
                .into()
            }));
        }
    }

    let data: Element<_> = column(data_lines).spacing(10).width(Length::Fill).into();

    return column![scrollable(data)]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(10)
        .align_items(Alignment::Start);
}

fn produce_script_edit_content<'a>(
    execution_lists: &execution_lists::ExecutionLists,
    visual_caches: &VisualCaches,
    edit_data: &EditData,
    app_config: &config::AppConfig,
    window_state: &WindowState,
) -> Column<'a, WindowMessage> {
    let Some(currently_edited_script) = &window_state.cursor_script else {
        return Column::new();
    };

    if currently_edited_script.script_type == EditScriptType::ScriptConfig {
        if edit_data.window_edit_data.is_none() {
            return Column::new();
        }
    }

    let edit_button = |label, message| {
        button(
            text(label)
                .vertical_alignment(alignment::Vertical::Center)
                .size(16),
        )
        .padding(4)
        .on_press(message)
    };

    let script = if currently_edited_script.script_type == EditScriptType::ScriptConfig {
        get_script_definition(&app_config, edit_data, currently_edited_script.idx)
    } else {
        &execution_lists.get_edited_execution_list()[currently_edited_script.idx]
    };

    let mut parameters: Vec<Element<'_, WindowMessage, iced::Renderer>> = Vec::new();

    match script {
        config::ScriptDefinition::Original(script) => {
            parameters.push(text("Name:").into());
            parameters.push(
                text_input("name", &script.name)
                    .on_input(move |new_arg| WindowMessage::EditScriptName(new_arg))
                    .padding(5)
                    .into(),
            );

            if currently_edited_script.script_type == EditScriptType::ScriptConfig {
                populate_path_editing_content(
                    "Command:",
                    "command",
                    &script.command,
                    &mut parameters,
                    |path| WindowMessage::EditScriptCommand(path),
                    |val| WindowMessage::ToggleScriptCommandRelativeToScripter(val),
                );

                populate_path_editing_content(
                    "Working directory:",
                    "path/to/directory",
                    &script.working_directory,
                    &mut parameters,
                    |path| WindowMessage::EditScriptWorkingDirectory(path),
                    |val| WindowMessage::ToggleScriptWorkingDirectoryRelativeToScripter(val),
                );

                populate_path_editing_content(
                    "Path to the icon:",
                    "path/to/icon.png",
                    &script.icon,
                    &mut parameters,
                    |path| WindowMessage::EditScriptIconPath(path),
                    |val| WindowMessage::ToggleScriptIconPathRelativeToScripter(val),
                );
            }

            parameters.push(
                text(
                    if currently_edited_script.script_type == EditScriptType::ExecutionList {
                        "Arguments line:"
                    } else {
                        "Default arguments:"
                    },
                )
                .into(),
            );
            parameters.push(
                text_input(&script.arguments_hint, &script.arguments)
                    .on_input(move |new_value| WindowMessage::EditArguments(new_value))
                    .style(
                        if currently_edited_script.script_type == EditScriptType::ExecutionList
                            && is_original_script_missing_arguments(&script)
                        {
                            theme::TextInput::Custom(Box::new(style::InvalidInputStyleSheet))
                        } else {
                            theme::TextInput::Default
                        },
                    )
                    .padding(5)
                    .id(ARGUMENTS_INPUT_ID.clone())
                    .into(),
            );
            if currently_edited_script.script_type == EditScriptType::ScriptConfig {
                parameters.push(
                    checkbox(
                        "Arguments are required",
                        script.requires_arguments,
                        move |val| WindowMessage::ToggleRequiresArguments(val),
                    )
                    .into(),
                );

                parameters.push(text("Argument hint:").into());
                parameters.push(
                    text_input("", &script.arguments_hint)
                        .on_input(move |new_value| WindowMessage::EditArgumentsHint(new_value))
                        .padding(5)
                        .into(),
                );
            }

            parameters.push(text("Retry count:").into());
            parameters.push(
                text_input("0", &visual_caches.autorerun_count)
                    .on_input(move |new_value| WindowMessage::EditAutorerunCount(new_value))
                    .padding(5)
                    .into(),
            );

            parameters.push(
                checkbox(
                    "Ignore previous failures",
                    script.ignore_previous_failures,
                    move |val| WindowMessage::ToggleIgnoreFailures(val),
                )
                .into(),
            );

            if currently_edited_script.script_type == EditScriptType::ScriptConfig {
                parameters.push(
                    edit_button(
                        "Duplicate script",
                        WindowMessage::DuplicateConfigScript(currently_edited_script.clone()),
                    )
                    .into(),
                );
            }

            if is_local_edited_script(
                currently_edited_script.idx,
                &app_config,
                &edit_data.window_edit_data,
            ) {
                parameters.push(
                    edit_button(
                        "Make shared",
                        WindowMessage::MoveToShared(currently_edited_script.clone()),
                    )
                    .into(),
                );
            }

            parameters.push(
                edit_button(
                    "Remove script",
                    WindowMessage::RemoveScript(currently_edited_script.clone()),
                )
                .style(theme::Button::Destructive)
                .into(),
            );
        }
        config::ScriptDefinition::ReferenceToShared(_, is_hidden) => {
            parameters.push(
                checkbox("Is script hidden", *is_hidden, move |val| {
                    WindowMessage::ToggleScriptHidden(val)
                })
                .into(),
            );

            if currently_edited_script.script_type == EditScriptType::ScriptConfig {
                parameters.push(
                    edit_button(
                        "Edit as a copy",
                        WindowMessage::CreateCopyOfSharedScript(currently_edited_script.clone()),
                    )
                    .into(),
                );

                parameters.push(
                    edit_button(
                        "Edit original",
                        WindowMessage::SwitchToOriginalSharedScript(
                            currently_edited_script.clone(),
                        ),
                    )
                    .into(),
                );
            }
        }
        config::ScriptDefinition::Preset(preset) => {
            parameters.push(text("Preset name:").into());
            parameters.push(
                text_input("name", &preset.name)
                    .on_input(move |new_arg| WindowMessage::EditScriptName(new_arg))
                    .padding(5)
                    .into(),
            );

            populate_path_editing_content(
                "Path to the icon:",
                "path/to/icon.png",
                &preset.icon,
                &mut parameters,
                |path| WindowMessage::EditScriptIconPath(path),
                |val| WindowMessage::ToggleScriptIconPathRelativeToScripter(val),
            );

            if is_local_edited_script(
                currently_edited_script.idx,
                &app_config,
                &edit_data.window_edit_data,
            ) {
                parameters.push(
                    edit_button(
                        "Make shared",
                        WindowMessage::MoveToShared(currently_edited_script.clone()),
                    )
                    .into(),
                );
            }

            parameters.push(
                edit_button(
                    "Remove preset",
                    WindowMessage::RemoveScript(currently_edited_script.clone()),
                )
                .style(theme::Button::Destructive)
                .into(),
            );
        }
    }

    let content = column(parameters).spacing(10);

    return column![scrollable(content)]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(10)
        .align_items(Alignment::Start);
}

fn produce_config_edit_content<'a>(
    config: &config::AppConfig,
    window_edit: &WindowEditData,
) -> Column<'a, WindowMessage> {
    let rewritable_config = get_rewritable_config(&config, &window_edit.edit_type);

    let mut list_elements: Vec<Element<'_, WindowMessage, iced::Renderer>> = Vec::new();

    list_elements.push(
        checkbox(
            "Always on top (requires restart)",
            rewritable_config.always_on_top,
            move |val| WindowMessage::ConfigToggleAlwaysOnTop(val),
        )
        .into(),
    );
    list_elements.push(
        checkbox(
            "Window status reactions",
            rewritable_config.window_status_reactions,
            move |val| WindowMessage::ConfigToggleWindowStatusReactions(val),
        )
        .into(),
    );
    list_elements.push(
        checkbox(
            "Keep window size",
            rewritable_config.keep_window_size,
            move |val| WindowMessage::ConfigToggleKeepWindowSize(val),
        )
        .into(),
    );
    list_elements.push(
        checkbox(
            "Show script filter",
            rewritable_config.enable_script_filtering,
            move |val| WindowMessage::ConfigToggleScriptFiltering(val),
        )
        .into(),
    );
    list_elements.push(
        checkbox(
            "Allow edit custom title",
            rewritable_config.enable_title_editing,
            move |val| WindowMessage::ConfigToggleTitleEditing(val),
        )
        .into(),
    );
    list_elements.push(
        checkbox(
            "Use custom theme",
            rewritable_config.custom_theme.is_some(),
            move |val| WindowMessage::ConfigToggleUseCustomTheme(val),
        )
        .into(),
    );

    if let Some(_theme) = &rewritable_config.custom_theme {
        list_elements.push(text("Background:").into());
        list_elements.push(
            text_input("#000000", &window_edit.theme_color_background)
                .on_input(move |new_value| WindowMessage::ConfigEditThemeBackground(new_value))
                .padding(5)
                .into(),
        );
        list_elements.push(text("Accent:").into());
        list_elements.push(
            text_input("#000000", &window_edit.theme_color_text)
                .on_input(move |new_value| WindowMessage::ConfigEditThemeText(new_value))
                .padding(5)
                .into(),
        );
        list_elements.push(text("Primary:").into());
        list_elements.push(
            text_input("#000000", &window_edit.theme_color_primary)
                .on_input(move |new_value| WindowMessage::ConfigEditThemePrimary(new_value))
                .padding(5)
                .into(),
        );
        list_elements.push(text("Success:").into());
        list_elements.push(
            text_input("#000000", &window_edit.theme_color_success)
                .on_input(move |new_value| WindowMessage::ConfigEditThemeSuccess(new_value))
                .padding(5)
                .into(),
        );
        list_elements.push(text("Danger:").into());
        list_elements.push(
            text_input("#000000", &window_edit.theme_color_danger)
                .on_input(move |new_value| WindowMessage::ConfigEditThemeDanger(new_value))
                .padding(5)
                .into(),
        );
        list_elements.push(text("Caption text:").into());
        list_elements.push(
            text_input("#000000", &window_edit.theme_color_caption_text)
                .on_input(move |new_value| WindowMessage::ConfigEditThemeCaptionText(new_value))
                .padding(5)
                .into(),
        );
        list_elements.push(text("Error text:").into());
        list_elements.push(
            text_input("#000000", &window_edit.theme_color_error_text)
                .on_input(move |new_value| WindowMessage::ConfigEditThemeErrorText(new_value))
                .padding(5)
                .into(),
        );
    }

    if window_edit.edit_type == ConfigEditType::Shared {
        populate_path_editing_content(
            "Local config path:",
            "path/to/config.json",
            &config.local_config_path,
            &mut list_elements,
            |path| WindowMessage::ConfigEditLocalConfigPath(path),
            |val| WindowMessage::ConfigToggleLocalConfigPathRelativeToScripter(val),
        );
    }

    return column![scrollable(column(list_elements))]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(10)
        .align_items(Alignment::Start);
}

fn view_content<'a>(
    execution_lists: &execution_lists::ExecutionLists,
    variant: &PaneVariant,
    theme: &Theme,
    paths: &config::PathCaches,
    visual_caches: &VisualCaches,
    config: &config::AppConfig,
    edit_data: &EditData,
    window_state: &WindowState,
) -> Element<'a, WindowMessage> {
    let rewritable_config = get_rewritable_config_opt(&config, &edit_data.window_edit_data);

    let content = match variant {
        PaneVariant::ScriptList => produce_script_list_content(
            config,
            rewritable_config,
            edit_data,
            &visual_caches.icons,
            window_state,
            theme,
        ),
        PaneVariant::ExecutionList => produce_execution_list_content(
            execution_lists,
            paths,
            theme,
            &config.custom_title,
            &visual_caches,
            edit_data,
            rewritable_config,
            window_state,
        ),
        PaneVariant::LogOutput => {
            produce_log_output_content(execution_lists, theme, rewritable_config)
        }
        PaneVariant::Parameters => match &edit_data.window_edit_data {
            Some(window_edit_data) if window_edit_data.is_editing_config => {
                produce_config_edit_content(config, window_edit_data)
            }
            _ => produce_script_edit_content(
                execution_lists,
                visual_caches,
                edit_data,
                config,
                window_state,
            ),
        },
    };

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(5)
        .center_y()
        .into()
}

fn view_controls<'a>(
    pane: pane_grid::Pane,
    variant: &PaneVariant,
    total_panes: usize,
    icons: &ui_icons::IconCaches,
    edit_data: &EditData,
    execution_lists: &execution_lists::ExecutionLists,
    is_maximized: bool,
    size: Size,
    window_state: &WindowState,
) -> Element<'a, WindowMessage> {
    let mut row = row![].spacing(5);

    if *variant == PaneVariant::ScriptList
        && !edit_data.window_edit_data.is_some()
        && !execution_lists.has_started_execution()
    {
        row = row.push(
            tooltip(
                edit_mode_button(
                    icons.themed.edit.clone(),
                    WindowMessage::EnterWindowEditMode,
                    edit_data.is_dirty,
                    window_state,
                ),
                "Enter editing mode",
                tooltip::Position::Left,
            )
            .style(theme::Container::Box),
        );
    }

    if total_panes > 1
        && (is_maximized
            || (*variant == PaneVariant::ExecutionList && execution_lists.has_started_execution()))
    {
        let toggle = {
            let (content, message) = if is_maximized {
                (
                    if window_state.is_command_key_down {
                        string_constants::UNFOCUS_COMMAND_HINT
                    } else {
                        "Restore full window"
                    },
                    WindowMessage::Restore,
                )
            } else {
                // adjust for window decorations
                let window_size = Size {
                    width: size.width + 3.0,
                    height: size.height + 3.0,
                };

                (
                    if window_state.is_command_key_down {
                        string_constants::FOCUS_COMMAND_HINT
                    } else {
                        "Focus"
                    },
                    WindowMessage::Maximize(pane, window_size),
                )
            };
            button(text(content).size(14))
                .style(theme::Button::Secondary)
                .padding(3)
                .on_press(message)
        };

        row = row.push(toggle);
    }

    row.into()
}

fn get_pane_name_from_variant(variant: &PaneVariant) -> &str {
    match variant {
        PaneVariant::ScriptList => "Scripts",
        PaneVariant::ExecutionList => "Execution",
        PaneVariant::LogOutput => "Log",
        PaneVariant::Parameters => "Parameters",
    }
}

fn apply_script_edit(
    app: &mut MainWindow,
    edit_fn: impl FnOnce(&mut config::OriginalScriptDefinition),
) {
    if let Some(script_id) = &app.window_state.cursor_script {
        match script_id.script_type {
            EditScriptType::ScriptConfig => match &app.edit_data.window_edit_data {
                Some(window_edit_data) if window_edit_data.edit_type == ConfigEditType::Local => {
                    if let Some(config) = &mut app.app_config.local_config_body {
                        match &mut config.script_definitions[script_id.idx] {
                            config::ScriptDefinition::Original(script) => {
                                edit_fn(script);
                                app.edit_data.is_dirty = true;
                                update_config_cache(&mut app.app_config, &app.edit_data);
                            }
                            _ => {}
                        }
                    }
                }
                _ => match &mut app.app_config.script_definitions[script_id.idx] {
                    config::ScriptDefinition::Original(script) => {
                        edit_fn(script);
                        app.edit_data.is_dirty = true;
                        update_config_cache(&mut app.app_config, &app.edit_data);
                    }
                    _ => {}
                },
            },
            EditScriptType::ExecutionList => {
                match &mut app.execution_data.get_edited_execution_list_mut()[script_id.idx] {
                    config::ScriptDefinition::Original(script) => {
                        edit_fn(script);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn get_theme(config: &config::AppConfig, window_edit_data: &Option<WindowEditData>) -> Theme {
    if let Some(theme) = get_rewritable_config_opt(&config, window_edit_data)
        .custom_theme
        .clone()
    {
        style::get_custom_theme(theme)
    } else {
        Theme::default()
    }
}

fn apply_theme_color_from_string(
    app: &mut MainWindow,
    color: String,
    set_theme_fn: impl FnOnce(&mut config::CustomTheme, [f32; 3]),
    set_text_fn: impl FnOnce(&mut WindowEditData, String) -> String,
) {
    if let Some(edit_data) = &mut app.edit_data.window_edit_data {
        let color_string = set_text_fn(edit_data, color);
        if let Some(custom_theme) =
            &mut get_rewritable_config_mut_non_opt(&mut app.app_config, edit_data).custom_theme
        {
            if let Some(new_color) = color_utils::hex_to_rgb(&color_string) {
                set_theme_fn(custom_theme, new_color);
                apply_theme(app);
                app.edit_data.is_dirty = true;
            }
        }
    }
}

fn get_rewritable_config<'a>(
    config: &'a config::AppConfig,
    edit_type: &ConfigEditType,
) -> &'a config::RewritableConfig {
    match edit_type {
        ConfigEditType::Shared => &config.rewritable,
        ConfigEditType::Local => {
            if let Some(local_config) = &config.local_config_body {
                &local_config.rewritable
            } else {
                &config.rewritable
            }
        }
    }
}

fn get_rewritable_config_opt<'a>(
    config: &'a config::AppConfig,
    edit_data: &Option<WindowEditData>,
) -> &'a config::RewritableConfig {
    match &edit_data {
        Some(edit_data) => get_rewritable_config(config, &edit_data.edit_type),
        None => {
            if let Some(local_config) = &config.local_config_body {
                &local_config.rewritable
            } else {
                &config.rewritable
            }
        }
    }
}

fn get_rewritable_config_mut<'a>(
    config: &'a mut config::AppConfig,
    window_edit: &Option<WindowEditData>,
) -> &'a mut config::RewritableConfig {
    return match window_edit {
        Some(window_edit) => get_rewritable_config_mut_non_opt(config, window_edit),
        None => &mut config.rewritable,
    };
}

fn get_rewritable_config_mut_non_opt<'a>(
    config: &'a mut config::AppConfig,
    window_edit: &WindowEditData,
) -> &'a mut config::RewritableConfig {
    match window_edit.edit_type {
        ConfigEditType::Shared => &mut config.rewritable,
        ConfigEditType::Local => {
            if let Some(local_config) = &mut config.local_config_body {
                &mut local_config.rewritable
            } else {
                &mut config.rewritable
            }
        }
    }
}

fn switch_config_edit_mode(app: &mut MainWindow, edit_type: ConfigEditType) {
    let is_config_editing = if let Some(window_edit) = &app.edit_data.window_edit_data {
        window_edit.is_editing_config
    } else {
        false
    };
    app.edit_data.window_edit_data = Some(WindowEditData::from_config(
        &app.app_config,
        is_config_editing,
        edit_type,
    ));
}

fn get_script_definition_list_opt<'a>(
    config: &'a config::AppConfig,
    window_edit_data: &Option<WindowEditData>,
) -> &'a Vec<config::ScriptDefinition> {
    match window_edit_data {
        Some(window_edit_data) => get_script_definition_list(config, &window_edit_data.edit_type),
        None => {
            if let Some(local_config) = &config.local_config_body {
                &local_config.script_definitions
            } else {
                &config.script_definitions
            }
        }
    }
}

fn get_script_definition_list<'a>(
    config: &'a config::AppConfig,
    edit_type: &ConfigEditType,
) -> &'a Vec<config::ScriptDefinition> {
    match edit_type {
        ConfigEditType::Shared => &config.script_definitions,
        ConfigEditType::Local => {
            if let Some(local_config) = &config.local_config_body {
                &local_config.script_definitions
            } else {
                &config.script_definitions
            }
        }
    }
}

fn apply_theme(app: &mut MainWindow) {
    app.theme = get_theme(&app.app_config, &app.edit_data.window_edit_data);
    update_theme_icons(app);
}

fn update_theme_icons(app: &mut MainWindow) {
    let icons = &mut app.visual_caches.icons;
    if app.theme.extended_palette().primary.strong.text.r > 0.5 {
        icons.themed = icons.bright.clone()
    } else {
        icons.themed = icons.dark.clone();
    }
}

fn is_local_edited_script(
    script_idx: usize,
    app_config: &config::AppConfig,
    window_edit_data: &Option<WindowEditData>,
) -> bool {
    if let Some(window_edit_data) = &window_edit_data {
        if window_edit_data.edit_type == ConfigEditType::Local {
            if let Some(scripts) = &app_config.local_config_body {
                match scripts.script_definitions.get(script_idx) {
                    Some(config::ScriptDefinition::Original(_)) => {
                        return true;
                    }
                    Some(config::ScriptDefinition::Preset(_)) => {
                        return true;
                    }
                    _ => {}
                }
            }
        }
    }
    return false;
}

fn add_script_to_shared_config(
    app_config: &mut config::AppConfig,
    script: config::ScriptDefinition,
) -> usize {
    app_config.script_definitions.push(script);
    let script_idx = app_config.script_definitions.len() - 1;
    config::populate_shared_scripts_from_config(app_config);
    return script_idx;
}

fn add_script_to_local_config(
    app_config: &mut config::AppConfig,
    edit_data: &EditData,
    script: config::ScriptDefinition,
) -> Option<usize> {
    if let Some(config) = &mut app_config.local_config_body {
        config.script_definitions.push(script);
    } else {
        return None;
    }

    update_config_cache(app_config, edit_data);

    return if let Some(config) = &mut app_config.local_config_body {
        Some(config.script_definitions.len() - 1)
    } else {
        None
    };
}

fn is_script_missing_arguments(script: &config::ScriptDefinition) -> bool {
    match script {
        config::ScriptDefinition::Original(script) => is_original_script_missing_arguments(script),
        _ => false,
    }
}

fn is_original_script_missing_arguments(script: &config::OriginalScriptDefinition) -> bool {
    script.requires_arguments && script.arguments.is_empty()
}

fn populate_path_editing_content(
    caption: &str,
    hint: &str,
    path: &config::PathConfig,
    edit_content: &mut Vec<Element<'_, WindowMessage, iced::Renderer>>,
    on_path_changed: impl Fn(String) -> WindowMessage + 'static,
    on_path_type_changed: impl Fn(bool) -> WindowMessage + 'static,
) {
    edit_content.push(text(caption).into());
    edit_content.push(
        text_input(hint, &path.path)
            .on_input(on_path_changed)
            .padding(5)
            .into(),
    );
    edit_content.push(
        checkbox(
            "Path relative to scripter executable",
            path.path_type == config::PathType::ScripterExecutableRelative,
            on_path_type_changed,
        )
        .into(),
    );
}

fn make_script_copy(script: config::ScriptDefinition) -> config::ScriptDefinition {
    match script {
        config::ScriptDefinition::ReferenceToShared(_, _) => script,
        config::ScriptDefinition::Preset(preset) => {
            config::ScriptDefinition::Preset(config::ScriptPreset {
                uid: config::Guid::new(),
                name: format!("{} (copy)", preset.name),
                ..preset
            })
        }
        config::ScriptDefinition::Original(script) => {
            config::ScriptDefinition::Original(config::OriginalScriptDefinition {
                uid: config::Guid::new(),
                name: format!("{} (copy)", script.name),
                ..script
            })
        }
    }
}

fn update_config_cache(app_config: &mut config::AppConfig, edit_data: &EditData) {
    let is_looking_at_local_config = if let Some(window_edit_data) = &edit_data.window_edit_data {
        window_edit_data.edit_type == ConfigEditType::Local
    } else {
        app_config.local_config_body.is_some()
    };

    let binding = edit_data.script_filter.to_lowercase();
    let search_words = binding.split_whitespace().collect::<Vec<&str>>();

    let is_full_list = edit_data.window_edit_data.is_some();

    let is_script_filtered_out = |name: &str| -> bool {
        !search_words.is_empty() && {
            let mut is_filtered_out = false;
            let lowercase_name = name.to_lowercase();
            for search_word in &search_words {
                if !lowercase_name.contains(search_word) {
                    is_filtered_out = true;
                    break;
                }
            }
            is_filtered_out
        }
    };

    let result_list = &mut app_config.displayed_configs_list_cache;
    let paths = &app_config.paths;
    if is_looking_at_local_config {
        let local_config = app_config.local_config_body.as_ref().unwrap();
        let shared_script_definitions = &app_config.script_definitions;

        result_list.clear();
        for script_definition in &local_config.script_definitions {
            match script_definition {
                config::ScriptDefinition::ReferenceToShared(shared_script_uid, is_hidden) => {
                    let shared_script =
                        shared_script_definitions
                            .iter()
                            .find(|script| match script {
                                config::ScriptDefinition::Original(script) => {
                                    script.uid == *shared_script_uid
                                }
                                config::ScriptDefinition::Preset(preset) => {
                                    preset.uid == *shared_script_uid
                                }
                                _ => false,
                            });
                    match shared_script {
                        Some(shared_script) => {
                            let name = match &shared_script {
                                config::ScriptDefinition::ReferenceToShared(_, _) => {
                                    "[Error]".to_string()
                                }
                                config::ScriptDefinition::Original(script) => script.name.clone(),
                                config::ScriptDefinition::Preset(preset) => preset.name.clone(),
                            };
                            let icon = match &shared_script {
                                config::ScriptDefinition::ReferenceToShared(_, _) => {
                                    config::PathConfig::default()
                                }
                                config::ScriptDefinition::Original(script) => script.icon.clone(),
                                config::ScriptDefinition::Preset(preset) => preset.icon.clone(),
                            };
                            let is_script_hidden = *is_hidden || is_script_filtered_out(&name);
                            if is_full_list || !is_script_hidden {
                                result_list.push(config::ScriptListCacheRecord {
                                    name,
                                    full_icon_path: config::get_full_optional_path(paths, &icon),
                                    is_hidden: is_script_hidden,
                                    original_script_uid: shared_script_uid.clone(),
                                });
                            }
                        }
                        None => {
                            eprintln!(
                                "Failed to find shared script with uid {}",
                                shared_script_uid.data
                            )
                        }
                    }
                }
                config::ScriptDefinition::Original(script) => {
                    let is_script_hidden = is_script_filtered_out(&script.name);
                    if is_full_list || !is_script_hidden {
                        result_list.push(config::ScriptListCacheRecord {
                            name: script.name.clone(),
                            full_icon_path: config::get_full_optional_path(paths, &script.icon),
                            is_hidden: is_script_hidden,
                            original_script_uid: script.uid.clone(),
                        });
                    }
                }
                config::ScriptDefinition::Preset(preset) => {
                    let is_script_hidden = is_script_filtered_out(&preset.name);

                    if is_full_list || !is_script_hidden {
                        result_list.push(config::ScriptListCacheRecord {
                            name: preset.name.clone(),
                            full_icon_path: config::get_full_optional_path(paths, &preset.icon),
                            is_hidden: is_script_hidden,
                            original_script_uid: preset.uid.clone(),
                        });
                    }
                }
            }
        }
    } else {
        let script_definitions = &app_config.script_definitions;

        result_list.clear();
        for script_definition in script_definitions {
            match script_definition {
                config::ScriptDefinition::ReferenceToShared(_, _) => {}
                config::ScriptDefinition::Original(script) => {
                    let is_script_hidden = is_script_filtered_out(&script.name);
                    if is_full_list || !is_script_hidden {
                        result_list.push(config::ScriptListCacheRecord {
                            name: script.name.clone(),
                            full_icon_path: config::get_full_optional_path(paths, &script.icon),
                            is_hidden: is_script_hidden,
                            original_script_uid: script.uid.clone(),
                        });
                    }
                }
                config::ScriptDefinition::Preset(preset) => {
                    let is_script_hidden = is_script_filtered_out(&preset.name);
                    if is_full_list || !is_script_hidden {
                        result_list.push(config::ScriptListCacheRecord {
                            name: preset.name.clone(),
                            full_icon_path: config::get_full_optional_path(paths, &preset.icon),
                            is_hidden: is_script_hidden,
                            original_script_uid: preset.uid.clone(),
                        });
                    }
                }
            }
        }
    }
}

fn get_script_definition<'a>(
    app_config: &'a config::AppConfig,
    edit_data: &EditData,
    script_idx: usize,
) -> &'a config::ScriptDefinition {
    let is_looking_at_local_config = if let Some(window_edit_data) = &edit_data.window_edit_data {
        window_edit_data.edit_type == ConfigEditType::Local
    } else {
        app_config.local_config_body.is_some()
    };

    return if is_looking_at_local_config {
        &app_config
            .local_config_body
            .as_ref()
            .unwrap()
            .script_definitions[script_idx]
    } else {
        &app_config.script_definitions[script_idx]
    };
}

fn get_script_definition_mut<'a>(
    app_config: &'a mut config::AppConfig,
    edit_data: &EditData,
    script_idx: usize,
) -> &'a mut config::ScriptDefinition {
    let is_looking_at_local_config = if let Some(window_edit_data) = &edit_data.window_edit_data {
        window_edit_data.edit_type == ConfigEditType::Local
    } else {
        app_config.local_config_body.is_some()
    };

    return if is_looking_at_local_config {
        &mut app_config
            .local_config_body
            .as_mut()
            .unwrap()
            .script_definitions[script_idx]
    } else {
        &mut app_config.script_definitions[script_idx]
    };
}

fn add_script_to_config(app: &mut MainWindow, script: config::ScriptDefinition) {
    if let Some(window_edit_data) = &app.edit_data.window_edit_data {
        let script_idx = match window_edit_data.edit_type {
            ConfigEditType::Shared => {
                Some(add_script_to_shared_config(&mut app.app_config, script))
            }
            ConfigEditType::Local => {
                add_script_to_local_config(&mut app.app_config, &app.edit_data, script)
            }
        };

        app.edit_data
            .window_edit_data
            .as_mut()
            .unwrap()
            .is_editing_config = false;

        if let Some(script_idx) = script_idx {
            select_edited_script(app, script_idx);
            app.edit_data.is_dirty = true;
        }
    }
}

fn get_editing_preset<'a>(
    app_config: &'a mut config::AppConfig,
    edit_data: &EditData,
    window_state: &WindowState,
) -> Option<&'a mut config::ScriptPreset> {
    if let Some(script_id) = &window_state.cursor_script {
        if script_id.script_type == EditScriptType::ScriptConfig {
            let script_definition = get_script_definition_mut(app_config, edit_data, script_id.idx);
            if let config::ScriptDefinition::Preset(preset) = script_definition {
                return Some(preset);
            }
        }
    }
    return None;
}

fn enter_window_edit_mode(app: &mut MainWindow) {
    app.edit_data.window_edit_data = Some(WindowEditData::from_config(
        &app.app_config,
        false,
        if app.app_config.local_config_body.is_some() {
            ConfigEditType::Local
        } else {
            ConfigEditType::Shared
        },
    ));
    app.edit_data.script_filter = String::new();
    clean_script_selection(&mut app.window_state.cursor_script);
    update_config_cache(&mut app.app_config, &app.edit_data);
    app.visual_caches.is_custom_title_editing = false;
}

fn exit_window_edit_mode(app: &mut MainWindow) {
    app.edit_data.window_edit_data = None;
    clean_script_selection(&mut app.window_state.cursor_script);
    apply_theme(app);
    update_config_cache(&mut app.app_config, &app.edit_data);
}

fn run_scheduled_scripts(app: &mut MainWindow) {
    if app.execution_data.get_edited_execution_list().is_empty() {
        return;
    }

    if app
        .execution_data
        .get_edited_execution_list()
        .iter()
        .any(|script| is_script_missing_arguments(script))
    {
        return;
    }

    if !app.execution_data.has_started_execution() {
        app.visual_caches.recent_logs.clear();
    }
    clean_script_selection(&mut app.window_state.cursor_script);
    app.execution_data.start_execution(&app.app_config);

    app.edit_data.script_filter = String::new();
    update_config_cache(&mut app.app_config, &app.edit_data);
}

fn add_script_to_execution(
    app: &mut MainWindow,
    script_uid: config::Guid,
    should_focus: bool,
) -> bool {
    let original_script =
        config::get_original_script_definition_by_uid(&app.app_config, script_uid);

    let original_script = if let Some(original_script) = original_script {
        original_script
    } else {
        return false;
    };

    match original_script {
        config::ScriptDefinition::ReferenceToShared(_, _) => {
            return false;
        }
        config::ScriptDefinition::Original(_) => {
            app.execution_data
                .add_script_to_execution(original_script.clone());
        }
        config::ScriptDefinition::Preset(preset) => {
            for preset_item in &preset.items {
                if let Some(script) = config::get_original_script_definition_by_uid(
                    &app.app_config,
                    preset_item.uid.clone(),
                ) {
                    let mut new_script = script.clone();

                    match &mut new_script {
                        config::ScriptDefinition::Original(script) => {
                            if let Some(name) = &preset_item.name {
                                script.name = name.clone();
                            }

                            if let Some(arguments) = &preset_item.arguments {
                                script.arguments = arguments.clone();
                            }

                            if let Some(autorerun_count) = preset_item.autorerun_count {
                                script.autorerun_count = autorerun_count;
                            }

                            if let Some(ignore_previous_failures) =
                                preset_item.ignore_previous_failures
                            {
                                script.ignore_previous_failures = ignore_previous_failures;
                            }
                        }
                        _ => {}
                    };

                    app.execution_data.add_script_to_execution(new_script);
                }
            }
        }
    }

    if should_focus {
        let script_idx = app.execution_data.get_edited_execution_list().len() - 1;
        select_execution_script(app, script_idx);
        app.window_state.pane_focus = Some(app.pane_by_pane_type[&PaneVariant::ExecutionList]);
    }

    return true;
}

fn focus_filter(app: &mut MainWindow) -> Command<WindowMessage> {
    if app.panes.maximized().is_none() {
        if let Some(focus) = app.window_state.pane_focus {
            if &app.panes.panes[&focus].variant != &PaneVariant::ScriptList {
                app.window_state.pane_focus = Some(app.pane_by_pane_type[&PaneVariant::ScriptList]);
            }
        } else {
            app.window_state.pane_focus = Some(app.pane_by_pane_type[&PaneVariant::ScriptList]);
        }
    }
    return text_input::focus(FILTER_INPUT_ID.clone());
}

fn clear_edited_scripts(app: &mut MainWindow) {
    if app.execution_data.has_started_execution() {
        return;
    }
    app.execution_data.clear_edited_scripts();
    clean_script_selection(&mut app.window_state.cursor_script);
}

fn clear_execution_scripts(app: &mut MainWindow) {
    if app.execution_data.has_started_execution()
        && (!app.execution_data.has_finished_execution()
            || app.execution_data.is_waiting_execution_to_finish())
    {
        return;
    }

    app.execution_data.clear_execution_scripts();
    clean_script_selection(&mut app.window_state.cursor_script);
}

fn select_edited_script(app: &mut MainWindow, script_idx: usize) {
    set_selected_script(
        &mut app.window_state.cursor_script,
        &app.execution_data.get_edited_execution_list(),
        &get_script_definition_list_opt(&app.app_config, &app.edit_data.window_edit_data),
        &mut app.visual_caches,
        script_idx,
        EditScriptType::ScriptConfig,
    );
    if let Some(window_edit_data) = &mut app.edit_data.window_edit_data {
        window_edit_data.is_editing_config = false;
    }
}

fn select_execution_script(app: &mut MainWindow, script_idx: usize) {
    set_selected_script(
        &mut app.window_state.cursor_script,
        &app.execution_data.get_edited_execution_list(),
        &app.execution_data.get_edited_execution_list(),
        &mut app.visual_caches,
        script_idx,
        EditScriptType::ExecutionList,
    );
}

fn select_script_by_type(app: &mut MainWindow, script_idx: usize, script_type: EditScriptType) {
    match script_type {
        EditScriptType::ScriptConfig => select_edited_script(app, script_idx),
        EditScriptType::ExecutionList => select_execution_script(app, script_idx),
    }
}

fn move_config_script_up(app: &mut MainWindow, index: usize) {
    if let Some(window_edit_data) = &mut app.edit_data.window_edit_data {
        match window_edit_data.edit_type {
            ConfigEditType::Shared => {
                if index >= 1 && index < app.app_config.script_definitions.len() {
                    app.app_config.script_definitions.swap(index, index - 1);
                    app.edit_data.is_dirty = true;
                }
            }
            ConfigEditType::Local => {
                if let Some(local_config_body) = &mut app.app_config.local_config_body {
                    if index >= 1 && index < local_config_body.script_definitions.len() {
                        local_config_body.script_definitions.swap(index, index - 1);
                        app.edit_data.is_dirty = true;
                    }
                }
            }
        }
    }

    if let Some(edited_script) = &app.window_state.cursor_script {
        if edited_script.idx == index && index > 0 {
            select_edited_script(app, index - 1);
        }
    }

    update_config_cache(&mut app.app_config, &app.edit_data);
}

fn move_config_script_down(app: &mut MainWindow, index: usize) {
    if let Some(window_edit_data) = &mut app.edit_data.window_edit_data {
        match window_edit_data.edit_type {
            ConfigEditType::Shared => {
                if index + 1 < app.app_config.script_definitions.len() {
                    app.app_config.script_definitions.swap(index, index + 1);
                    app.edit_data.is_dirty = true;
                }
            }
            ConfigEditType::Local => {
                if let Some(local_config_body) = &mut app.app_config.local_config_body {
                    if index + 1 < local_config_body.script_definitions.len() {
                        local_config_body.script_definitions.swap(index, index + 1);
                        app.edit_data.is_dirty = true;
                    }
                }
            }
        }
    }

    if let Some(edited_script) = &app.window_state.cursor_script {
        if edited_script.idx == index
            && index + 1 < app.app_config.displayed_configs_list_cache.len()
        {
            select_edited_script(app, index + 1);
        }
    }

    update_config_cache(&mut app.app_config, &app.edit_data);
}

fn move_cursor(app: &mut MainWindow, is_up: bool) {
    let focused_pane = if let Some(focus) = app.window_state.pane_focus {
        app.panes.panes[&focus].variant
    } else {
        return;
    };

    if focused_pane == PaneVariant::ScriptList || focused_pane == PaneVariant::ExecutionList {
        let pane_script_type = match focused_pane {
            PaneVariant::ScriptList => EditScriptType::ScriptConfig,
            PaneVariant::ExecutionList => EditScriptType::ExecutionList,
            _ => unreachable!(),
        };

        let scripts_count = match focused_pane {
            PaneVariant::ScriptList => app.app_config.displayed_configs_list_cache.len(),
            PaneVariant::ExecutionList => app.execution_data.get_edited_execution_list().len(),
            _ => unreachable!(),
        };

        if scripts_count == 0 {
            return;
        }

        let cursor_script_type = app
            .window_state
            .cursor_script
            .as_ref()
            .map(|x| x.script_type);
        let cursor_script_idx = app.window_state.cursor_script.as_ref().map(|x| x.idx);

        let next_selection = if cursor_script_idx.is_none()
            || (cursor_script_idx.is_some() && cursor_script_type != Some(pane_script_type))
        {
            if is_up {
                scripts_count - 1
            } else {
                0
            }
        } else {
            let cursor_script_idx = cursor_script_idx.unwrap_or_default();
            if is_up {
                if cursor_script_idx > 0 {
                    cursor_script_idx - 1
                } else {
                    scripts_count - 1
                }
            } else {
                if cursor_script_idx + 1 < scripts_count {
                    cursor_script_idx + 1
                } else {
                    0
                }
            }
        };

        select_script_by_type(app, next_selection, pane_script_type);
    }
}

fn get_next_pane_selection(app: &MainWindow, is_forward: bool) -> PaneVariant {
    if let Some(focus) = app.window_state.pane_focus {
        // try to predict what the user wants to do

        let is_editing = app.edit_data.window_edit_data.is_some();
        let selected_script_type = app
            .window_state
            .cursor_script
            .as_ref()
            .map(|s| &s.script_type);

        let have_scripts_in_execution = !app.execution_data.get_edited_execution_list().is_empty();
        let have_parameters_open = if let Some(selected_script_type) = selected_script_type {
            selected_script_type == &EditScriptType::ExecutionList || is_editing
        } else {
            false
        };
        let circle_clockwise = if let Some(selected_script_type) = selected_script_type {
            let editing_script =
                selected_script_type == &EditScriptType::ScriptConfig && is_editing;
            editing_script != is_forward
        } else {
            is_forward
        };

        if &app.panes.panes[&focus].variant == &PaneVariant::ScriptList {
            if !have_scripts_in_execution || !have_parameters_open {
                if !have_scripts_in_execution && !have_parameters_open {
                    PaneVariant::ScriptList
                } else if !have_scripts_in_execution {
                    PaneVariant::Parameters
                } else {
                    PaneVariant::ExecutionList
                }
            } else if circle_clockwise {
                PaneVariant::ExecutionList
            } else {
                PaneVariant::Parameters
            }
        } else if &app.panes.panes[&focus].variant == &PaneVariant::ExecutionList {
            if !have_parameters_open {
                PaneVariant::ScriptList
            } else if circle_clockwise {
                PaneVariant::Parameters
            } else {
                PaneVariant::ScriptList
            }
        } else if &app.panes.panes[&focus].variant == &PaneVariant::Parameters {
            if !have_scripts_in_execution {
                PaneVariant::ScriptList
            } else if circle_clockwise {
                PaneVariant::ScriptList
            } else {
                PaneVariant::ExecutionList
            }
        } else {
            // if we're in the log pane, go to the script list
            PaneVariant::ScriptList
        }
    } else {
        // if no panes selected, select ScriptList
        PaneVariant::ScriptList
    }
}

fn remove_script(app: &mut MainWindow, script_id: &EditScriptId) {
    match script_id.script_type {
        EditScriptType::ScriptConfig => {
            if let Some(window_edit_data) = &mut app.edit_data.window_edit_data {
                match window_edit_data.edit_type {
                    ConfigEditType::Shared => {
                        app.app_config.script_definitions.remove(script_id.idx);
                        app.edit_data.is_dirty = true;
                    }
                    ConfigEditType::Local => {
                        if let Some(config) = &mut app.app_config.local_config_body {
                            config.script_definitions.remove(script_id.idx);
                            app.edit_data.is_dirty = true;
                        }
                    }
                }
            }

            config::populate_shared_scripts_from_config(&mut app.app_config);
            update_config_cache(&mut app.app_config, &app.edit_data);
        }
        EditScriptType::ExecutionList => {
            app.execution_data.remove_script(script_id.idx);
        }
    }
    clean_script_selection(&mut app.window_state.cursor_script);
}

fn maximize_pane(
    app: &mut MainWindow,
    pane: pane_grid::Pane,
    window_size: Size,
) -> Command<WindowMessage> {
    if app.window_state.pane_focus != Some(pane) {
        clean_script_selection(&mut app.window_state.cursor_script);
    }
    app.window_state.pane_focus = Some(pane);
    app.panes.maximize(&pane);
    app.window_state.has_maximized_pane = true;
    if !get_rewritable_config_opt(&app.app_config, &app.edit_data.window_edit_data).keep_window_size
    {
        app.window_state.full_window_size = window_size.clone();
        let size = app
            .panes
            .layout()
            .pane_regions(1.0, Size::new(window_size.width, window_size.height))
            .get(&pane)
            .unwrap() // tried to get an non-existing pane, this should never happen, so panic
            .clone();

        let scheduled_elements_count =
            app.execution_data.get_scheduled_execution_list().len() as u32;
        let edited_elements_count = app.execution_data.get_edited_execution_list().len() as u32;
        let title_lines = if let Some(custom_title) = app.app_config.custom_title.as_ref() {
            custom_title.lines().count() as u32
        } else {
            0
        };

        return resize(
            size.width as u32,
            std::cmp::min(
                size.height as u32,
                EMPTY_EXECUTION_LIST_HEIGHT
                    + edited_elements_count * ONE_EXECUTION_LIST_ELEMENT_HEIGHT
                    + scheduled_elements_count * ONE_EXECUTION_LIST_ELEMENT_HEIGHT
                    + title_lines * ONE_TITLE_LINE_HEIGHT
                    + if edited_elements_count > 0 {
                        EXTRA_EDIT_CONTENT_HEIGHT
                    } else {
                        0
                    },
            ),
        );
    }

    return Command::none();
}

fn restore_window(app: &mut MainWindow) -> Command<WindowMessage> {
    app.window_state.has_maximized_pane = false;
    app.panes.restore();
    if !get_rewritable_config_opt(&app.app_config, &app.edit_data.window_edit_data).keep_window_size
    {
        return resize(
            app.window_state.full_window_size.width as u32,
            app.window_state.full_window_size.height as u32,
        );
    }
    return Command::none();
}

fn find_best_shared_script_insert_position(
    source_script_definitions: &Vec<config::ScriptDefinition>,
    target_script_definitions: &Vec<config::ScriptDefinition>,
    script_id: &EditScriptId,
) -> usize {
    let script_idx = script_id.idx;

    // first search up to find if we have reference to shared scripts
    let mut last_shared_script_idx = script_idx;
    let mut target_shared_script_uid = config::GUID_NULL;
    for i in (0..script_idx).rev() {
        if let config::ScriptDefinition::ReferenceToShared(uid, _) = &source_script_definitions[i] {
            last_shared_script_idx = i;
            target_shared_script_uid = uid.clone();
            break;
        }
    }

    if last_shared_script_idx != script_idx {
        return find_script_idx_by_id(target_script_definitions, &target_shared_script_uid)
            .unwrap_or(target_script_definitions.len() - 1)
            + 1;
    }

    // search down
    let mut next_shared_script_idx = script_idx;
    let mut target_shared_script_idx = config::GUID_NULL;
    for i in script_idx..source_script_definitions.len() {
        if let config::ScriptDefinition::ReferenceToShared(uid, _) = &source_script_definitions[i] {
            next_shared_script_idx = i;
            target_shared_script_idx = uid.clone();
            break;
        }
    }

    if next_shared_script_idx != script_idx {
        return find_script_idx_by_id(target_script_definitions, &target_shared_script_idx)
            .unwrap_or(target_script_definitions.len());
    }

    // if we didn't find any shared scripts, just insert at the end
    return target_script_definitions.len();
}

fn find_script_idx_by_id(
    script_definitions: &Vec<config::ScriptDefinition>,
    script_id: &config::Guid,
) -> Option<usize> {
    for i in 0..script_definitions.len() {
        match &script_definitions[i] {
            config::ScriptDefinition::Original(script) => {
                if script.uid == *script_id {
                    return Some(i);
                }
            }
            config::ScriptDefinition::Preset(preset) => {
                if preset.uid == *script_id {
                    return Some(i);
                }
            }
            config::ScriptDefinition::ReferenceToShared(uid, _) => {
                if *uid == *script_id {
                    return Some(i);
                }
            }
        }
    }
    return None;
}

fn switch_to_editing_shared_config(app: &mut MainWindow) {
    clean_script_selection(&mut app.window_state.cursor_script);
    switch_config_edit_mode(app, ConfigEditType::Shared);
    apply_theme(app);
    update_config_cache(&mut app.app_config, &app.edit_data);
}

fn get_config_error_content<'a>(
    error: &config::ConfigReadError,
    _theme: &Theme,
) -> Column<'a, WindowMessage> {
    let mut content = Vec::new();
    content.push(text("Error:").into());
    match error {
        config::ConfigReadError::FileReadError { file_path, error } => {
            content.push(
                text(format!(
                    "Failed to read file '{}'",
                    file_path.to_string_lossy()
                ))
                .into(),
            );
            content.push(text("Make sure the file has correct access rights").into());
            content.push(text(format!("Details: {}", error)).into());
            if let Some(file_path) = file_path.parent() {
                content.push(
                    button("Open file location")
                        .on_press(WindowMessage::OpenWithDefaultApplication(
                            file_path.to_path_buf(),
                        ))
                        .into(),
                );
            }
        }
        config::ConfigReadError::DataParseJsonError { file_path, error } => {
            content.push(
                text(format!(
                    "Failed to parse JSON data from file '{}'",
                    file_path.to_string_lossy()
                ))
                .into(),
            );
            content.push(
                text("If you made any manual edits to the file, make sure they are correct").into(),
            );
            content.push(text(format!("Details: {}", error)).into());
            content.push(
                button("Open file")
                    .on_press(WindowMessage::OpenWithDefaultApplication(file_path.clone()))
                    .into(),
            );
            content.push(text("If you believe this is a bug in scripter, please report it").into());
            content.push(
                button("Report bug")
                    .on_press(WindowMessage::OpenUrl(
                        "https://github.com/gameraccoon/scripter/labels/bug".to_string(),
                    ))
                    .into(),
            );
        }
        config::ConfigReadError::UpdaterUnknownVersion {
            file_path,
            version,
            latest_version,
        } => {
            content.push(
                text(format!(
                    "Unknown version of the config file '{}'",
                    file_path.to_string_lossy()
                ))
                .into(),
            );
            content.push(
                text(format!(
                    "The version of the file is '{}', latest known version is '{}'",
                    version, latest_version
                ))
                .into(),
            );
            content.push(
                text("This version of Scripter might not be compatible with the file").into(),
            );
            content.push(text("Please update Scripter to the latest version").into());
            content.push(
                button("Open releases")
                    .on_press(WindowMessage::OpenUrl(
                        "https://github.com/gameraccoon/scripter/releases".to_string(),
                    ))
                    .into(),
            );
        }
        config::ConfigReadError::ConfigDeserializeError { file_path, error } => {
            content.push(
                text(format!(
                    "Failed to deserialize config file '{}'",
                    file_path.to_string_lossy()
                ))
                .into(),
            );
            content.push(
                text("If you made any manual edits to the file, make sure they are correct").into(),
            );
            content.push(text(format!("Details: {}", error)).into());
            content.push(
                button("Open file")
                    .on_press(WindowMessage::OpenWithDefaultApplication(file_path.clone()))
                    .into(),
            );
            content.push(text("If you believe this is a bug in scripter, please report it").into());
            content.push(
                button("Report bug")
                    .on_press(WindowMessage::OpenUrl(
                        "https://github.com/gameraccoon/scripter/labels/bug".to_string(),
                    ))
                    .into(),
            );
        }
        config::ConfigReadError::ConfigSerializeError { error } => {
            content.push(text("Failed to serialize a config file").into());
            content.push(
                text("This is likely a bug in Scripter, please report it to the developer").into(),
            );
            content.push(text(format!("Details: {}", error)).into());
            content.push(
                button("Report bug")
                    .on_press(WindowMessage::OpenUrl(
                        "https://github.com/gameraccoon/scripter/labels/bug".to_string(),
                    ))
                    .into(),
            );
        }
        config::ConfigReadError::FileWriteError { file_path, error } => {
            content.push(
                text(format!(
                    "Failed to write to file '{}'",
                    file_path.to_string_lossy()
                ))
                .into(),
            );
            content.push(
                text("Make sure the file has correct access rights and is not read-only").into(),
            );
            content.push(text(format!("Details: {}", error)).into());
            if let Some(file_path) = file_path.parent() {
                content.push(
                    button("Open file location")
                        .on_press(WindowMessage::OpenWithDefaultApplication(
                            file_path.to_path_buf(),
                        ))
                        .into(),
                );
            }
        }
    }

    content.push(text(format!("Application version {}", env!("CARGO_PKG_VERSION"))).into());
    return Column::with_children(content).spacing(10);
}
