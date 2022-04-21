use crate::{GuiCommand, Message};
use druid::{
    im::Vector,
    widget::{Controller, Image},
    Env, Event, EventCtx, ImageBuf, Widget,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct ImageController {
    pub dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>,
}

impl Controller<Message, Image> for ImageController {
    fn lifecycle(
        &mut self,
        child: &mut Image,
        _ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &Message,
        _env: &Env,
    ) {
        if let druid::LifeCycle::WidgetAdded = event {
            let link = &data.content;
            if let Some(id) = self.dled_images.lock().unwrap().get(link) {
                child.set_image_data(id.clone());
            }
        }
    }
}

pub struct ScrollController;

impl<W> Controller<Vector<Message>, druid::widget::Scroll<Vector<Message>, W>> for ScrollController
where
    W: Widget<Vector<Message>>,
{
    fn update(
        &mut self,
        child: &mut druid::widget::Scroll<Vector<Message>, W>,
        ctx: &mut druid::UpdateCtx,
        old_data: &Vector<Message>,
        data: &Vector<Message>,
        env: &Env,
    ) {
        //TODO: fix scroll...
        //  === notification??
        let should_scroll = !child.scroll_by(druid::Vec2 { x: 0.0, y: 0.01 });
        child.update(ctx, old_data, data, env);
        if should_scroll {
            child.scroll_by(druid::Vec2 { x: 0.0, y: 20.0 });
        }
    }
}

pub struct TakeFocusConnect;

impl<T: std::fmt::Debug, W: Widget<T>> Controller<T, W> for TakeFocusConnect {
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        if let Event::WindowConnected = event {
            ctx.request_focus();
        } else if let Event::Command(command) = event {
            if let Some(GuiCommand::ConnectionEnded(_)) =
                command.get::<GuiCommand>(druid::Selector::new("gui_command"))
            {
                ctx.request_focus();
            }
        }
        child.event(ctx, event, data, env)
    }
}

pub struct TakeFocusMain;

impl<T: std::fmt::Debug, W: Widget<T>> Controller<T, W> for TakeFocusMain {
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        if let Event::Command(command) = event {
            if let Some(GuiCommand::Connected) =
                command.get::<GuiCommand>(druid::Selector::new("gui_command"))
            {
                ctx.request_focus();
            }
        }
        child.event(ctx, event, data, env)
    }
}
