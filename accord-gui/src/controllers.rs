use crate::{GuiCommand, Message, GUI_COMMAND};
use druid::{
    im::Vector,
    widget::{Controller, Image},
    Env, Event, EventCtx, ImageBuf, Insets, Selector, Size, Widget, WidgetExt, WidgetPod,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

const LIST_CHANGED: Selector<Size> = Selector::new("list-changed");

pub const SCROLL: Selector<f64> = Selector::new("scroll");

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

    fn try_get_image(&mut self, link: &str) -> bool {
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
            return true;
        }
        false
    }
}

impl Widget<Message> for ImageFromLink {
    fn event(&mut self, ctx: &mut druid::EventCtx, event: &Event, data: &mut Message, env: &Env) {
        if let Event::Command(cmd) = event {
            if let Some(link_c) = cmd.get(Selector::<String>::new("image_downloaded")) {
                let link = &data.content;
                if link == link_c && self.try_get_image(link) {
                    ctx.children_changed();
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
        if let druid::LifeCycle::WidgetAdded = event {
            if self.try_get_image(&data.content) {
                ctx.children_changed();
            }
        }
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
    ) -> Size {
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

pub struct ScrollController {
    prev_child_size: Option<Size>,
    widget_added_time: std::time::Instant,
}

impl ScrollController {
    pub fn new() -> Self {
        Self {
            prev_child_size: None,
            widget_added_time: std::time::Instant::now(),
        }
    }
}

impl<W> Controller<Vector<Message>, druid::widget::Scroll<Vector<Message>, W>> for ScrollController
where
    W: Widget<Vector<Message>>,
{
    fn event(
        &mut self,
        child: &mut druid::widget::Scroll<Vector<Message>, W>,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut Vector<Message>,
        env: &Env,
    ) {
        if let Event::Command(cmd) = event {
            if let Some(size) = cmd.get(LIST_CHANGED) {
                let mut should_scroll = true;
                if let Some(prev_size) = self.prev_child_size.replace(*size) {
                    should_scroll =
                        (prev_size.height - (child.offset().y + ctx.size().height)).abs() < 50.0;
                }

                // To make sure it gets scrolled to the bottom at startup
                if self.widget_added_time.elapsed().as_secs() < 3 {
                    should_scroll = true;
                }
                if should_scroll {
                    child.scroll_by(druid::Vec2 { x: 0.0, y: 1e10 });
                    ctx.children_changed();
                }
            }
            if let Some(mult) = cmd.get(SCROLL) {
                const PG_SCROLL: f64 = 200.0;
                child.scroll_by(druid::Vec2 {
                    x: 0.0,
                    y: mult * PG_SCROLL,
                });
                ctx.children_changed();
            }
        }

        child.event(ctx, event, data, env)
    }

    fn lifecycle(
        &mut self,
        child: &mut druid::widget::Scroll<Vector<Message>, W>,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &Vector<Message>,
        env: &Env,
    ) {
        if let druid::LifeCycle::WidgetAdded = event {
            self.widget_added_time = std::time::Instant::now();
            child.scroll_by(druid::Vec2 { x: 0.0, y: 1e10 });
            ctx.children_changed();
        }
        child.lifecycle(ctx, event, data, env)
    }
}

pub struct ListController;

impl Controller<Vector<Message>, druid::widget::List<Message>> for ListController {
    fn lifecycle(
        &mut self,
        child: &mut druid::widget::List<Message>,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &Vector<Message>,
        env: &Env,
    ) {
        if let druid::LifeCycle::Size(size) = event {
            ctx.submit_command(LIST_CHANGED.with(*size));
        }
        child.lifecycle(ctx, event, data, env)
    }
}

pub struct TakeFocusConnect;

impl<T, W: Widget<T>> Controller<T, W> for TakeFocusConnect {
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        if let Event::WindowConnected = event {
            ctx.request_focus();
        } else if let Event::Command(command) = event {
            if let Some(GuiCommand::ConnectionEnded(_)) = command.get(GUI_COMMAND) {
                ctx.request_focus();
            }
        }
        child.event(ctx, event, data, env)
    }
}

pub struct TakeFocusMain;

impl<T, W: Widget<T>> Controller<T, W> for TakeFocusMain {
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        if let Event::Command(command) = event {
            if let Some(GuiCommand::Connected) = command.get(GUI_COMMAND) {
                ctx.request_focus();
            }
        }
        child.event(ctx, event, data, env)
    }
}

pub struct MessageTextBoxController;

impl<T, W: Widget<T>> Controller<T, W> for MessageTextBoxController {
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        if let Event::Paste(clipboard) = event {
            let supported_types = &["image/png", "image/jpeg"];
            let best_available_type = clipboard.preferred_format(supported_types);

            if let Some(format) = best_available_type {
                let data = clipboard
                    .get_format(format)
                    .expect("I promise not to unwrap in production");
                ctx.submit_command(GUI_COMMAND.with(GuiCommand::SendImage(Arc::new(data))));
            }
        }
        child.event(ctx, event, data, env)
    }
}
