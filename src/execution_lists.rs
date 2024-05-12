// Copyright (C) Pavel Grebnev 2023-2024
// Distributed under the MIT License (license terms are at http://opensource.org/licenses/MIT).

use chrono;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::config;
use crate::execution_thread;
use crate::file_utils;
use crate::ring_buffer;

// one list of scripts, an execution can have multiple lists
// in that case as soon as one list finishes, the next one starts immediately
// for the user it looks like one continuous execution
struct ExecutionList {
    execution_data: execution_thread::ScriptExecutionData,
    // in the cached lists, from which elment this list starts
    first_cache_index: usize,
}

// executions can be run in parallel, each of them tracks its own progress
pub struct Execution {
    execution_lists: Vec<ExecutionList>,
    current_execution_list: usize,

    scheduled_scripts_cache: Vec<config::ScriptDefinition>,
    scheduled_scripts_statuses: Vec<execution_thread::ScriptExecutionStatus>,

    has_failed_scripts: bool,

    log_directory: PathBuf,
    recent_logs: Arc<Mutex<execution_thread::LogBuffer>>,
    currently_outputting_script: isize,
}

// Here is an example diagram with 3 parallel executions running in total 9 scripts:
// started_executions: [
//   E0 : EL[S1, S2] => EL[S3] => EL[S4, S5]
//   E1 : EL[S11, S12]
//   E2 : EL[S21] => EL[S22, S23]
// ]

pub struct ExecutionLists {
    started_executions: Vec<Execution>,
    edited_execution_list: execution_thread::ScriptExecutionData,
}

impl Execution {
    pub fn new() -> Self {
        Self {
            execution_lists: Vec::new(),
            current_execution_list: 0,
            scheduled_scripts_cache: Vec::new(),
            scheduled_scripts_statuses: Vec::new(),
            has_failed_scripts: false,
            log_directory: PathBuf::new(),
            recent_logs: Arc::new(Mutex::new(ring_buffer::RingBuffer::new(Default::default()))),
            currently_outputting_script: -1,
        }
    }

    // either starts a new execution or adds a new list to the current execution
    pub fn execute_scripts(
        &mut self,
        app_config: &config::AppConfig,
        edited_execution_list: execution_thread::ScriptExecutionData,
    ) {
        if edited_execution_list.scripts_to_run.is_empty() {
            return;
        }

        let first_cache_index = self.scheduled_scripts_cache.len();
        let is_already_started = !self.execution_lists.is_empty();

        // we keep the cache to be able to display the list of scripts in the UI
        self.scheduled_scripts_cache
            .append(&mut edited_execution_list.scripts_to_run.clone());
        // append as many empty statuses as we added scripts
        self.scheduled_scripts_statuses.append(&mut vec![
            execution_thread::ScriptExecutionStatus {
                start_time: None,
                finish_time: None,
                result: execution_thread::ScriptResultStatus::Success,
                retry_count: 0,
            };
            edited_execution_list.scripts_to_run.len()
        ]);
        self.execution_lists.push(ExecutionList {
            execution_data: edited_execution_list,
            first_cache_index,
        });

        if !is_already_started {
            self.log_directory = file_utils::get_script_log_directory(
                &app_config.paths.logs_path,
                &chrono::Local::now(),
            );

            self.run_execution_list(app_config);
        }
    }

    pub fn request_stop_execution(&mut self) {
        if self.current_execution_list < self.execution_lists.len() {
            execution_thread::request_stop_execution(
                &mut self.execution_lists[self.current_execution_list].execution_data,
            );
        }
    }

    pub fn is_waiting_execution_to_finish(&self) -> bool {
        if self.execution_lists.is_empty() {
            return false;
        }
        if let Some(execution_list) = self.execution_lists.get(self.current_execution_list - 1) {
            return execution_list.execution_data.is_waiting_execution_thread_to_finish();
        }
        return false;
    }

    pub fn has_finished_execution(&self) -> bool {
        if let Some(scheduled_script) = self.scheduled_scripts_statuses.last() {
            return scheduled_script.has_script_finished();
        }
        return false;
    }

    pub fn get_log_path(&self) -> &PathBuf {
        &self.log_directory
    }

    pub fn get_currently_outputting_script(&self) -> isize {
        self.currently_outputting_script
    }

    pub fn get_scheduled_scripts_cache(&self) -> &Vec<config::ScriptDefinition> {
        &self.scheduled_scripts_cache
    }

    pub fn get_scheduled_scripts_statuses(&self) -> &Vec<execution_thread::ScriptExecutionStatus> {
        &self.scheduled_scripts_statuses
    }

    pub fn tick(&mut self, app_config: &config::AppConfig) -> bool {
        let current_execution_list = &mut self.execution_lists[self.current_execution_list];
        if let Some(rx) = &current_execution_list.execution_data.progress_receiver {
            if let Ok(progress) = rx.try_recv() {
                if progress.1.has_script_failed() {
                    self.has_failed_scripts = true;
                }
                let script_local_idx = progress.0;
                let script_status = progress.1;

                let execution_list = &mut self.execution_lists[self.current_execution_list];

                let script_idx = execution_list.first_cache_index + script_local_idx;

                self.scheduled_scripts_statuses[script_idx] = script_status;
                self.currently_outputting_script = progress.0 as isize;

                if self.scheduled_scripts_statuses[script_idx].has_script_finished()
                    && execution_list.execution_data.scripts_to_run.len() == script_local_idx + 1
                {
                    self.current_execution_list += 1;
                    if self.try_join_execution_thread(self.current_execution_list - 1) {
                        self.run_execution_list(app_config);
                    }
                }

                if self.has_finished_execution() {
                    return true;
                }
            }
        } else {
            if self.current_execution_list > 0 {
                if self.try_join_execution_thread(self.current_execution_list - 1) {
                    self.run_execution_list(app_config);
                }
            }
        }
        return false;
    }

    fn run_execution_list(&mut self, app_config: &config::AppConfig) {
        if self.current_execution_list + 1 >= self.execution_lists.len() {
            return;
        }

        let had_failures_before =
            if let Some(last_script) = self.get_previous_execution_list_status() {
                last_script.has_script_failed() || last_script.has_script_been_skipped()
            } else {
                false
            };

        let execution_list = &mut self.execution_lists[self.current_execution_list];

        if execution_list.execution_data.scripts_to_run.is_empty() {
            return;
        }

        execution_thread::run_scripts(
            &mut execution_list.execution_data,
            &self.log_directory,
            had_failures_before,
            &app_config,
            self.recent_logs.clone(),
            execution_list.first_cache_index,
        );
    }

    fn get_previous_execution_list_status(&self) -> Option<&execution_thread::ScriptExecutionStatus> {
        if self.current_execution_list > 0 {
            let previous_execution_list = &self.execution_lists[self.current_execution_list - 1];
            Some(
                &self.scheduled_scripts_statuses[previous_execution_list.first_cache_index
                    + previous_execution_list.execution_data.scripts_to_run.len()
                    - 1],
            )
        } else {
            None
        }
    }

    fn try_join_execution_thread(&mut self, list_idx: usize) -> bool {
        if let Some(handle) = &self.execution_lists[list_idx]
            .execution_data
            .thread_join_handle
        {
            if handle.is_finished() {
                self.join_execution_thread(list_idx);
                return true;
            }
        } else {
            // the thread has already been joined
            return true;
        }

        return false;
    }

    fn join_execution_thread(&mut self, list_idx: usize) {
        // this should never block, since the thread should be finished by now
        // but we do it anyway not to miss bugs that create zombie threads
        if let Some(join_handle) = self.execution_lists[list_idx]
            .execution_data
            .thread_join_handle
            .take()
        {
            join_handle.join().unwrap(); // have no idea what to do if this fails, crashing is probably fine
        };
    }
}

impl ExecutionLists {
    pub fn new() -> Self {
        Self {
            started_executions: Vec::new(),
            edited_execution_list: execution_thread::ScriptExecutionData::new(),
        }
    }

    pub fn add_script_to_edited_list(&mut self, script: config::ScriptDefinition) {
        execution_thread::add_script_to_execution(&mut self.get_edited_scripts_mut(), script);
    }

    pub fn remove_script_from_edited_list(&mut self, idx: usize) {
        execution_thread::remove_script_from_execution(&mut self.get_edited_scripts_mut(), idx);
    }

    pub fn get_edited_scripts(&self) -> &Vec<config::ScriptDefinition> {
        &self.edited_execution_list.scripts_to_run
    }

    pub fn get_edited_scripts_mut(&mut self) -> &mut Vec<config::ScriptDefinition> {
        &mut self.edited_execution_list.scripts_to_run
    }

    pub fn take_edited_scripts(&mut self) -> execution_thread::ScriptExecutionData {
        std::mem::replace(
            &mut self.edited_execution_list,
            execution_thread::ScriptExecutionData::new(),
        )
    }

    pub fn clear_edited_scripts(&mut self) {
        self.get_edited_scripts_mut().clear();
    }

    pub fn get_execution_lists(&self) -> &Vec<Execution> {
        &self.started_executions
    }

    pub fn start_new_execution(&mut self, app_config: &config::AppConfig) {
        self.started_executions.push(Execution::new());

        let new_execution_list = std::mem::replace(
            &mut self.edited_execution_list,
            execution_thread::ScriptExecutionData::new(),
        );
        self.started_executions
            .last_mut()
            .unwrap()
            .execute_scripts(app_config, new_execution_list);
    }

    pub fn tick(&mut self, app_config: &config::AppConfig) -> bool {
        let mut has_unfinished_executions = false;
        for execution in &mut self.started_executions {
            if execution.has_finished_execution() {
                continue;
            }
            let is_finished = execution.tick(app_config);
            if !is_finished {
                has_unfinished_executions = true;
            }
        }
        return !has_unfinished_executions;
    }

    pub fn has_any_execution_started(&self) -> bool {
        !self.started_executions.is_empty()
    }

    pub fn has_all_executions_finished(&self) -> bool {
        self.started_executions
            .iter()
            .all(|execution| execution.has_finished_execution())
    }

    pub fn has_any_execution_failed(&self) -> bool {
        self.started_executions
            .iter()
            .any(|execution| execution.has_failed_scripts)
    }
}
