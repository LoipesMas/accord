use crate::{GuiCommand, Message};
use druid::{
    im::Vector,
    widget::{Controller, Image},
    Env, Event, EventCtx, ImageBuf, Insets, Widget, WidgetExt, WidgetPod,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

// "Heavily inspired" by RemoteImage from jpochyla's psst ;]
pub struct ImageFromLink {
    pub dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>,
    placeholder: WidgetPod<Message, Box<dyn Widget<Message>>>,
    image: Option<WidgetPod<Message, Box<dyn Widget<Message>>>>,
}
impl ImageFromLink {
    pub fn new(
        placeholder: impl Widget<Message> + 'static,
        dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>,
    ) -> Self {
        Self {
            placeholder: WidgetPod::new(placeholder).boxed(),
            dled_images,
            image: None,
        }
    }
}

impl Widget<Message> for ImageFromLink {
    fn event(&mut self, ctx: &mut druid::EventCtx, event: &Event, data: &mut Message, env: &Env) {
        if let Event::Command(cmd) = event {
            if let Some(link_c) = cmd.get(druid::Selector::<String>::new("image_downloaded")) {
                let link = &data.content;
                if link == link_c {
                    if let Some(ib) = self.dled_images.lock().unwrap().get(link) {
                        self.image.replace(
                            WidgetPod::new(
                                Image::new(ib.clone())
                                    .fill_mode(druid::widget::FillStrat::Contain)
                                    .interpolation_mode(druid::piet::InterpolationMode::Bilinear)
                                    .fix_width(400.0)
                                    .align_left()
                                    .padding(Insets::uniform_xy(50.0, 0.0)),
                            )
                            .boxed(),
                        );
                        ctx.children_changed();
                    }
                }
                return;
            }
        }

        if let Some(image) = self.image.as_mut() {
            image.event(ctx, event, data, env);
        } else {
            self.placeholder.event(ctx, event, data, env);
        }
    }
    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &Message,
        env: &Env,
    ) {
        if let Some(image) = self.image.as_mut() {
            image.lifecycle(ctx, event, data, env);
        } else {
            self.placeholder.lifecycle(ctx, event, data, env);
        }
    }
    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        _old_data: &Message,
        data: &Message,
        env: &Env,
    ) {
        // if we ever add message editing, we need to update this!
        //
        if let Some(image) = self.image.as_mut() {
            image.update(ctx, data, env);
        } else {
            self.placeholder.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &Message,
        env: &Env,
    ) -> druid::Size {
        if let Some(image) = self.image.as_mut() {
            let size = image.layout(ctx, bc, data, env);
            image.set_origin(ctx, data, env, druid::Point::ORIGIN);
            size
        } else {
            let size = self.placeholder.layout(ctx, bc, data, env);
            self.placeholder
                .set_origin(ctx, data, env, druid::Point::ORIGIN);
            size
        }
    }

    fn paint(&mut self, ctx: &mut druid::PaintCtx, data: &Message, env: &Env) {
        if let Some(image) = self.image.as_mut() {
            image.paint(ctx, data, env)
        } else {
            self.placeholder.paint(ctx, data, env)
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
