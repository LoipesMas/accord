use druid::{
    kurbo::{PathEl, Shape},
    Color, Data, Env, KeyOrValue, PaintCtx, Point, Rect, RenderContext, Size, Widget, WidgetPod,
};

pub trait WidgetExt2<T: Data>: Widget<T> + Sized + 'static {
    fn cut_corners(
        self,
        bottom_left_offset: f64,
        bottom_right_offset: f64,
        top_right_offset: f64,
        top_left_offset: f64,
    ) -> CutCorners<T> {
        CutCorners::new(
            bottom_left_offset,
            bottom_right_offset,
            top_right_offset,
            top_left_offset,
            self,
        )
    }

    fn cut_corners_sym(self, offset: f64) -> CutCorners<T> {
        CutCorners::new_sym(offset, self)
    }
}

impl<T: Data, W: Widget<T> + 'static> WidgetExt2<T> for W {}

struct BorderStyle {
    width: KeyOrValue<f64>,
    color: KeyOrValue<Color>,
}

pub struct CutCorners<T> {
    bottom_left_offset: f64,
    bottom_right_offset: f64,
    top_right_offset: f64,
    top_left_offset: f64,
    inner: WidgetPod<T, Box<dyn Widget<T>>>,
    background: Option<druid::widget::BackgroundBrush<T>>,
    border: Option<BorderStyle>,
}

impl<T> CutCorners<T> {
    pub fn new(
        bottom_left_offset: f64,
        bottom_right_offset: f64,
        top_right_offset: f64,
        top_left_offset: f64,
        inner: impl Widget<T> + 'static,
    ) -> Self {
        Self {
            bottom_left_offset,
            bottom_right_offset,
            top_right_offset,
            top_left_offset,
            inner: WidgetPod::new(inner).boxed(),
            background: None,
            border: None,
        }
    }

    #[allow(dead_code)]
    pub fn new_sym(offset: f64, inner: impl Widget<T> + 'static) -> Self {
        Self {
            bottom_left_offset: offset,
            bottom_right_offset: offset,
            top_right_offset: offset,
            top_left_offset: offset,
            inner: WidgetPod::new(inner).boxed(),
            background: None,
            border: None,
        }
    }

    pub fn with_background(
        mut self,
        background: impl Into<druid::widget::BackgroundBrush<T>>,
    ) -> Self {
        self.background = Some(background.into());
        self
    }

    pub fn with_border(
        mut self,
        color: impl Into<KeyOrValue<Color>>,
        width: impl Into<KeyOrValue<f64>>,
    ) -> Self {
        self.border = Some(BorderStyle {
            color: color.into(),
            width: width.into(),
        });
        self
    }

    fn _cut_corners(&self, rect: Rect) -> CutCornersRect {
        CutCornersRect::new(
            rect,
            self.bottom_left_offset,
            self.bottom_right_offset,
            self.top_right_offset,
            self.top_left_offset,
        )
    }
}

impl<T: Data> Widget<T> for CutCorners<T> {
    fn event(&mut self, ctx: &mut druid::EventCtx, event: &druid::Event, data: &mut T, env: &Env) {
        self.inner.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &T,
        env: &Env,
    ) {
        self.inner.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut druid::UpdateCtx, _old_data: &T, data: &T, env: &Env) {
        self.inner.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        bc.debug_check("CutCorners");
        let size = self.inner.layout(ctx, bc, data, env);
        let my_size = size;
        let origin = Point::ZERO;
        self.inner.set_origin(ctx, data, env, origin);

        let my_insets = self.inner.compute_parent_paint_insets(my_size);
        ctx.set_paint_insets(my_insets);
        my_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let panel = self._cut_corners(ctx.size().to_rect());
        if let Some(background) = self.background.as_mut() {
            ctx.with_save(|ctx| {
                ctx.clip(panel.clone());
                background.paint(ctx, data, env);
            });
        }

        if let Some(border) = self.border.as_ref() {
            let border_width = border.width.resolve(env);
            ctx.with_save(|ctx| {
                ctx.clip(panel.clone());
                ctx.stroke(panel, &border.color.resolve(env), border_width);
            })
        }
        self.inner.paint(ctx, data, env);
    }
}

#[derive(Clone)]
struct CutCornersRect {
    rect: Rect,
    bottom_left_offset: f64,
    bottom_right_offset: f64,
    top_right_offset: f64,
    top_left_offset: f64,
}

impl CutCornersRect {
    fn new(
        rect: Rect,
        bottom_left_offset: f64,
        bottom_right_offset: f64,
        top_right_offset: f64,
        top_left_offset: f64,
    ) -> Self {
        Self {
            rect,
            bottom_left_offset,
            bottom_right_offset,
            top_right_offset,
            top_left_offset,
        }
    }

    fn _offsets(&self) -> [&f64; 4] {
        [
            &self.top_left_offset,
            &self.top_right_offset,
            &self.bottom_right_offset,
            &self.bottom_left_offset,
        ]
    }
}

struct CutCornersRectPathIter {
    idx: usize,
    points: [Point; 8],
}

impl Iterator for CutCornersRectPathIter {
    type Item = PathEl;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = if self.idx == 0 {
            Some(PathEl::MoveTo(self.points[0]))
        } else if self.idx == 8 {
            Some(PathEl::ClosePath)
        } else if self.idx >= 9 {
            None
        } else {
            Some(PathEl::LineTo(self.points[self.idx]))
        };
        self.idx += 1;
        ret
    }
}

impl Shape for CutCornersRect {
    type PathElementsIter = CutCornersRectPathIter;

    fn path_elements(&self, _tolerance: f64) -> Self::PathElementsIter {
        CutCornersRectPathIter {
            idx: 0,
            points: [
                Point::new(self.top_left_offset, 0.0),
                Point::new(self.rect.width() - self.top_right_offset, 0.0),
                Point::new(self.rect.width(), self.top_right_offset),
                Point::new(
                    self.rect.width(),
                    self.rect.height() - self.bottom_right_offset,
                ),
                Point::new(
                    self.rect.width() - self.bottom_right_offset,
                    self.rect.height(),
                ),
                Point::new(self.bottom_left_offset, self.rect.height()),
                Point::new(0.0, self.rect.height() - self.bottom_left_offset),
                Point::new(0.0, self.top_left_offset),
            ],
        }
    }

    fn area(&self) -> f64 {
        let rect_area = self.rect.area();

        let triangles_areas = self
            ._offsets()
            .iter()
            .fold(0.0, |accum, item| accum + item.powi(2) / 2.0);
        rect_area - triangles_areas
    }

    fn perimeter(&self, accuracy: f64) -> f64 {
        let mut perim = self.rect.perimeter(accuracy);
        for offset in self._offsets() {
            perim -= 2.0 * offset;
            perim += offset * std::f64::consts::SQRT_2;
        }
        perim
    }

    fn winding(&self, _pt: Point) -> i32 {
        todo!("Is it even used?")
    }

    fn bounding_box(&self) -> Rect {
        self.rect
    }
}
