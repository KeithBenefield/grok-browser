// src/webview.rs
use wry::{
    application::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::WindowBuilder,
    },
    webview::WebViewBuilder,
};

pub fn setup_webview() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Grok Browser")
        .with_inner_size(wry::application::dpi::LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .expect("Failed to create window");

    let webview = WebViewBuilder::new(window)
        .expect("Failed to create WebViewBuilder")
        .with_url("https://grok.com")
        .expect("Failed to set URL")
        .with_initialization_script(
            r#"
            setTimeout(() => {
                let input = document.querySelector('input[type="text"][class*="query"]') || 
                            document.querySelector('input[type="text"][placeholder*="know"]') || 
                            document.querySelector('textarea[class*="query"]');
                if (input) {
                    input.value = 'Hello, Grok!';
                    let button = document.querySelector('button[class*="think"]') || 
                                document.querySelector('button[aria-label="Think"]') || 
                                document.querySelector('button');
                    if (button) {
                        button.click();
                    } else {
                        console.log('Submit button not found');
                    }
                } else {
                    console.log('Query input not found');
                }
            }, 2000);
            "#,
        )
        .with_devtools(true)
        .build()
        .expect("Failed to build WebView");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {
                let _ = &webview; // Reference webview to suppress warning
            }
        }
    });
}