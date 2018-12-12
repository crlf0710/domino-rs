#[macro_use]
extern crate log;

#[allow(dead_code)]
mod command_queue {
    use std::collections::VecDeque;

    pub(crate) struct CommandList<T> {
        current_frame: VecDeque<T>,
        stashed_frames: Vec<VecDeque<T>>,
    }

    impl<T> CommandList<T> {
        pub fn new() -> Self {
            CommandList {
                current_frame: VecDeque::new(),
                stashed_frames: Vec::new(),
            }
        }

        pub fn pop_command(&mut self) -> Option<T> {
            self.current_frame.pop_front()
        }

        pub fn pop_command_and_maybe_frame(&mut self) -> Option<T> {
            loop {
                if let Some(v) = self.current_frame.pop_front() {
                    return Some(v);
                }
                if !self.maybe_pop_frame() {
                    return None;
                }
            }
        }

        pub fn push_immediate_command(&mut self, new_command: T) {
            self.current_frame.push_front(new_command);
        }

        pub fn add_command_to_current_frame(&mut self, new_command: T) {
            self.current_frame.push_back(new_command);
        }

        pub fn append_commands_to_current_frame<I: IntoIterator<Item = T>>(&mut self, new_commands: I) {
            self.current_frame.extend(new_commands);
        }

        pub fn add_command_to_bottom_frame(&mut self, new_command: T) {
            let bottom_frame = self
                .stashed_frames
                .get_mut(0)
                .unwrap_or(&mut self.current_frame);
            bottom_frame.push_back(new_command);
        }

        pub fn append_commands_to_bottom_frame<I: IntoIterator<Item = T>>(&mut self, new_commands: I) {
            let bottom_frame = self
                .stashed_frames
                .get_mut(0)
                .unwrap_or(&mut self.current_frame);
            bottom_frame.extend(new_commands);
        }


        pub fn start_new_frame(&mut self) {
            use std::mem::swap;
            let mut new_frame = VecDeque::new();
            swap(&mut new_frame, &mut self.current_frame);
            self.stashed_frames.push(new_frame);
            debug_assert!(self.current_frame.is_empty());
        }

        pub fn maybe_pop_frame(&mut self) -> bool {
            if !self.current_frame.is_empty() {
                false
            } else if let Some(mut old_frame) = self.stashed_frames.pop() {
                use std::mem::swap;
                swap(&mut self.current_frame, &mut old_frame);
                true
            } else {
                false
            }
        }

        pub fn is_empty(&self) -> bool {
            self.current_frame.is_empty() && self.stashed_frames.is_empty()
        }

        pub fn is_current_frame_empty(&self) -> bool {
            self.current_frame.is_empty()
        }

        pub fn is_only_frame(&self) -> bool {
            self.stashed_frames.is_empty()
        }
    }
}

pub mod mvc {
    use command_queue::CommandList;
    use std::fmt::Debug;

    #[derive(Debug)]
    enum MVCMessage<M: Model<V, C>, V: View<M, C>, C: Controller<M, V>> {
        ModelCommand(M::Command),
        ModelUpdateView(M::Notification),
        ViewCommand(V::Command),
        ControllerManipulatesModel(C::Notification),
        ControllerCommand(C::Command),
    }

    pub struct MVCSystem<M: Model<V, C>, V: View<M, C>, C: Controller<M, V>> {
        model: M,
        view: V,
        controller: C,
        command_list: CommandList<MVCMessage<M, V, C>>,
    }

    impl<M, V, C> MVCSystem<M, V, C>
        where M: Model<V, C>, V: View<M, C>, C: Controller<M, V>
    {
        pub fn new(model: M, view: V, controller: C) -> Self {
            MVCSystem {
                model,
                view,
                controller,
                command_list: CommandList::new(),
            }
        }

        pub fn model(&self) -> &M {
            &self.model
        }

        pub fn view(&self) -> &V {
            &self.view
        }

        pub fn controller(&self) -> &C {
            &self.controller
        }

        pub fn process_input(
            &mut self,
            input: impl Into<C::Command>,
        ) {
            self
                .command_list
                .add_command_to_bottom_frame(MVCMessage::ControllerCommand(input.into()));
            self.exec_pending_commands();
        }

        pub fn redirect_output_target(&mut self, target: Option<V::OutputTarget>) {
            self.view.redirect_output_target(target);
            self.exec_pending_commands();
        }

        pub fn sync_output(&self) where V::OutputParameter : Default {
            let mut param = Default::default();
            self.view.sync_output_with_parameter(&self.model, &mut param);
        }

        pub fn sync_output_with_parameter(&self, param: &mut V::OutputParameter) {
            self.view.sync_output_with_parameter(&self.model, param);
        }

        fn exec_immediate_command(&mut self, command: MVCMessage<M, V, C>) {
            match command {
                MVCMessage::ModelCommand(model_command) => {
                    self.command_list.start_new_frame();
                    let model_token = ModelToken{system: self};
                    M::process_command(model_token, model_command);
                },
                MVCMessage::ModelUpdateView(model_notification) => {
                    if let Some(view_command) = V::translate_model_notification(model_notification) {
                        self.exec_immediate_command(MVCMessage::ViewCommand(view_command));
                    }
                },
                MVCMessage::ViewCommand(view_command) => {
                    self.command_list.start_new_frame();
                    let view_token = ViewToken{system: self};
                    V::process_command(view_token, view_command);
                },
                MVCMessage::ControllerManipulatesModel(controller_notification) => {
                    if let Some(model_command) = M::translate_controller_notification(controller_notification) {
                        self.exec_immediate_command(MVCMessage::ModelCommand(model_command));
                    }
                },
                MVCMessage::ControllerCommand(controller_command) => {
                    self.command_list.start_new_frame();
                    let controller_token = ControllerToken{system: self};
                    C::process_command(controller_token, controller_command);
                },
            }
        }

        fn exec_immediate_command_in_new_frame(&mut self, command: MVCMessage<M, V, C>) {
            self.command_list.start_new_frame();
            self.exec_immediate_command(command);
        }

        fn exec_pending_commands(&mut self) {
            while let Some(command) = self.command_list.pop_command_and_maybe_frame() {
                self.exec_immediate_command(command);
            }
        }
    }


    pub struct ModelToken<'a, M: Model<V, C>, V: View<M, C>, C: Controller<M, V>> {
        system: &'a mut MVCSystem<M, V, C>,
    }

    impl<'a, M, V, C> ModelToken<'a, M, V, C>
        where M: Model<V, C>, V: View<M, C>, C: Controller<M, V> {

        pub fn model(&self) -> &M {
            &self.system.model
        }

        pub fn model_mut(&mut self) -> &mut M {
            &mut self.system.model
        }

        pub fn exec_command_now(&mut self, command: M::Command) {
            self.system.exec_immediate_command_in_new_frame(MVCMessage::ModelCommand(command));
        }

        pub fn exec_command_next(&mut self, command: M::Command) {
            self.system.command_list.add_command_to_current_frame(MVCMessage::ModelCommand(command));
        }

        pub fn exec_command_later(&mut self, command: M::Command) {
            self.system.command_list.add_command_to_bottom_frame(MVCMessage::ModelCommand(command));
        }

        pub fn update_view_now(&mut self, notification: M::Notification) {
            self.system.exec_immediate_command_in_new_frame(MVCMessage::ModelUpdateView(notification));
        }

        pub fn update_view_next(&mut self, notification: M::Notification) {
            self.system.command_list.add_command_to_current_frame(MVCMessage::ModelUpdateView(notification))
        }

        pub fn update_view_later(&mut self, notification: M::Notification) {
            self.system.command_list.add_command_to_bottom_frame(MVCMessage::ModelUpdateView(notification))
        }
    }

    pub struct ViewToken<'a, M: Model<V, C>, V: View<M, C>, C: Controller<M, V>> {
        system: &'a mut MVCSystem<M, V, C>,
    }

    impl<'a, M, V, C> ViewToken<'a, M, V, C>
        where M: Model<V, C>, V: View<M, C>, C: Controller<M, V> {

        pub fn view(&self) -> &V {
            &self.system.view
        }

        pub fn view_mut(&mut self) -> &mut V {
            &mut self.system.view
        }

        pub fn model(&self) -> &M {
            &self.system.model
        }

        pub fn exec_command_now(&mut self, command: V::Command) {
            self.system.exec_immediate_command_in_new_frame(MVCMessage::ViewCommand(command));
        }

        pub fn exec_command_next(&mut self, command: V::Command) {
            self.system.command_list.add_command_to_current_frame(MVCMessage::ViewCommand(command));
        }

        pub fn exec_command_later(&mut self, command: V::Command) {
            self.system.command_list.add_command_to_bottom_frame(MVCMessage::ViewCommand(command));
        }

        pub fn redirect_output_target(&mut self, target: Option<V::OutputTarget>) {
            self.system.view.redirect_output_target(target);
        }

        pub fn sync_output(&self) where V::OutputParameter : Default {
            let mut param = Default::default();
            self.system.view.sync_output_with_parameter(&self.system.model, &mut param);
        }

        pub fn sync_output_with_parameter(&self, param: &mut V::OutputParameter) {
            self.system.view.sync_output_with_parameter(&self.system.model, param);
        }

    }


    pub struct ControllerToken<'a, M: Model<V, C>, V: View<M, C>, C: Controller<M, V>> {
        system: &'a mut MVCSystem<M, V, C>,
    }


    impl<'a, M, V, C> ControllerToken<'a, M, V, C>
        where M: Model<V, C>, V: View<M, C>, C: Controller<M, V> {

        pub fn controller(&self) -> &C {
            &self.system.controller
        }

        pub fn controller_mut(&mut self) -> &mut C {
            &mut self.system.controller
        }

        pub fn exec_command_now(&mut self, command: C::Command) {
            self.system.exec_immediate_command_in_new_frame(MVCMessage::ControllerCommand(command));
        }

        pub fn exec_command_next(&mut self, command: C::Command) {
            self.system.command_list.add_command_to_current_frame(MVCMessage::ControllerCommand(command));
        }

        pub fn exec_command_later(&mut self, command: C::Command) {
            self.system.command_list.add_command_to_bottom_frame(MVCMessage::ControllerCommand(command));
        }

        pub fn manipulate_model_now(&mut self, notification: C::Notification) {
            self.system.exec_immediate_command_in_new_frame(MVCMessage::ControllerManipulatesModel(notification));
        }

        pub fn manipulate_model_next(&mut self, notification: C::Notification) {
            self.system.command_list.add_command_to_current_frame(MVCMessage::ControllerManipulatesModel(notification))
        }

        pub fn manipulate_model_later(&mut self, notification: C::Notification) {
            self.system.command_list.add_command_to_bottom_frame(MVCMessage::ControllerManipulatesModel(notification))
        }
    }


    pub trait Model<V: View<Self, C>, C: Controller<Self, V>>: Sized + 'static {
        type Command: Debug;
        type Notification: Debug;

        #[allow(unused_variables)]
        fn process_command(token: ModelToken<Self, V, C>, command: Self::Command) {
            debug!("Executing model command: {:?}", command);
        }

        fn translate_controller_notification(controller_notification: C::Notification) -> Option<Self::Command> {
            debug!("Translating controller notification to model command: {:?} -> {:?}", controller_notification, "None");
            None
        }
    }

    pub trait View<M: Model<Self, C>, C: Controller<M, Self>>: Sized + 'static {
        type Command: Debug;
        type OutputTarget;
        type OutputParameter;

        #[allow(unused_variables)]
        fn process_command(token: ViewToken<M, Self, C>, command: Self::Command) {
            debug!("Executing view command: {:?}", command);
        }

        fn translate_model_notification(model_notification: M::Notification) -> Option<Self::Command> {
            debug!("Translating model notification to view command: {:?} -> {:?}", model_notification, "None");
            None
        }

        fn redirect_output_target(&mut self, target: Option<Self::OutputTarget>) {
            debug!("Redirecting output target to: {}", if target.is_some() { "<target>"} else { "None"});
        }

        #[allow(unused_variables)]
        fn sync_output_with_parameter(
            &self, model: &M, parameter: &mut Self::OutputParameter
        ) {
            debug!("Sync output");
        }

    }

    pub trait Controller<M: Model<V, Self>, V: View<M, Self>>: Sized + 'static {
        type Command: Debug;
        type Notification: Debug;

        #[allow(unused_variables)]
        fn process_command(token: ControllerToken<M, V, Self>, command: Self::Command) {
            debug!("Executing controller command: {:?}", command);
        }
    }

}
