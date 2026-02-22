use iced::{
    Border, Center, Color, Element, Fill,
    alignment::Vertical,
    border::Radius,
    widget::{center, container, mouse_area, opaque, stack, text},
};

/// Creates a modal that can be used to display content above
/// the given `base` [Element].
pub fn modal<'a, Message>(
    base: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    stack![
        base.into(),
        opaque(
            mouse_area(center(opaque(content)).style(|_theme| {
                container::Style {
                    background: Some(
                        Color {
                            a: 0.8,
                            ..Color::BLACK
                        }
                        .into(),
                    ),
                    ..container::Style::default()
                }
            }))
            .on_press(on_blur)
        )
    ]
    .into()
}

/// Overlays a temporary snackbar notification at the bottom of `base`.
///
/// `anim_height` drives the slide animation: pass 0.0 to hide, and
/// `SNACKBAR_MAX_HEIGHT` (or any positive value ≥ the toast's natural height)
/// to fully reveal the toast. Values in between wipe the toast in from the
/// bottom edge, creating a slide-in / slide-out effect.
pub fn snackbar<'a, Message>(
    base: impl Into<Element<'a, Message>>,
    message: String,
    background: Color,
    anim_height: f32,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    /*let bg_color = match kind {
        SnackbarKind::Error => Color {
            r: 0.75,
            g: 0.15,
            b: 0.15,
            a: 0.75,
        },
        SnackbarKind::Success => Color {
            r: 0.1,
            g: 0.6,
            b: 0.2,
            a: 0.75,
        },
    };*/

    let toast = container(text(message).color(Color::WHITE))
        .padding([8, 14])
        .style(move |_theme| container::Style {
            background: Some(background.into()),
            border: Border {
                radius: Radius::new(6),
                ..Border::default()
            },
            ..container::Style::default()
        });

    // The toast is anchored to the bottom of this container.
    // As anim_height grows from 0 → full toast height, the toast wipes up
    // from the bottom edge — giving a slide-in effect. Reversing slides out.
    let clip_wrapper = container(toast)
        .align_y(Vertical::Bottom)
        .clip(true)
        .height(anim_height);

    let overlay = container(clip_wrapper)
        .align_x(Center)
        .align_y(Vertical::Bottom)
        .width(Fill)
        .height(Fill)
        .padding(16);

    stack![base.into(), overlay].into()
}
