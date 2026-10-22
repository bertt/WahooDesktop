//! Native window (WebView2) that talks to the in-process router; no TCP port is opened.
use crate::{app::App, server};
use axum::{Router, body::Body};
use std::sync::Arc;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use tower::ServiceExt;
use wry::{WebViewBuilder, http::Response};

async fn dispatch(router: Router, req: wry::http::Request<Vec<u8>>) -> Response<Vec<u8>> {
    let (parts, body) = req.into_parts();
    let req = wry::http::Request::from_parts(parts, Body::from(body));
    match router.oneshot(req).await {
        Ok(resp) => {
            let (parts, body) = resp.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
            Response::from_parts(parts, bytes.to_vec())
        }
        Err(never) => match never {},
    }
}

pub fn run(app: Arc<App>) {
    let rt = Arc::new(tokio::runtime::Runtime::new().expect("tokio runtime"));
    let router = server::router(app);
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Wahoo Desktop")
        .with_window_icon(tao::platform::windows::IconExtWindows::from_resource(1, None).ok())
        .with_inner_size(LogicalSize::new(1360.0, 860.0))
        .build(&event_loop)
        .expect("window");

    let handle = rt.clone();
    let _webview = WebViewBuilder::new()
        .with_asynchronous_custom_protocol("app".into(), move |_id, req, responder| {
            let router = router.clone();
            handle.spawn(async move { responder.respond(dispatch(router, req).await) });
        })
        .with_url(format!("http://app.localhost/{}", std::env::var("WAHOO_START_HASH").unwrap_or_default()))
        .with_devtools(true)
        .build(&window)
        .expect("webview");

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *flow = ControlFlow::Exit;
        }
    });
}
