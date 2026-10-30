//! Precision touchpad input, including the inertia supplied by Windows.

#![allow(non_snake_case)]

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use windows::core::{implement, Ref, Result};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::DirectManipulation::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Input::Pointer::GetPointerType;
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, KillTimer, SetTimer, PT_TOUCHPAD};

use crate::dpi::{PhysicalPosition, PhysicalSize};
use crate::event::{MouseScrollDelta, TouchPhase, WindowEvent};

use super::DEVICE_ID;

pub(super) const UPDATE_TIMER_ID: usize = 0xF17A_1001;

/// Lives on the window's event-loop thread. COM callbacks only queue input;
/// application callbacks run after the native update has returned.
pub(super) struct DirectManipulation {
    window: HWND,
    manager: IDirectManipulationManager,
    update_manager: IDirectManipulationUpdateManager,
    viewport: IDirectManipulationViewport,
    handler_cookie: Cell<Option<u32>>,
    gesture: Arc<Mutex<Gesture>>,
    size: Cell<PhysicalSize<u32>>,
    timer_running: Cell<bool>,
    updating: Cell<bool>,
    activated: Cell<bool>,
    closed: Cell<bool>,
    // Release the COM interfaces before balancing CoInitializeEx.
    _apartment: Apartment,
}

impl DirectManipulation {
    pub(super) fn new(window: windows_sys::Win32::Foundation::HWND) -> Result<Self> {
        let apartment = Apartment::new()?;
        let window = HWND(window);
        let manager: IDirectManipulationManager =
            unsafe { CoCreateInstance(&DirectManipulationManager, None, CLSCTX_INPROC_SERVER)? };
        let update_manager = unsafe { manager.GetUpdateManager()? };
        let viewport = unsafe { manager.CreateViewport(None, window)? };
        let gesture = Arc::new(Mutex::new(Gesture::default()));
        let helper = Self {
            window,
            manager,
            update_manager,
            viewport,
            handler_cookie: Cell::new(None),
            gesture: gesture.clone(),
            size: Cell::new(PhysicalSize::new(1, 1)),
            timer_running: Cell::new(false),
            updating: Cell::new(false),
            activated: Cell::new(false),
            closed: Cell::new(false),
            _apartment: apartment,
        };

        unsafe {
            // Let Windows choose touchpad rails and inertia.
            helper.viewport.ActivateConfiguration(
                DIRECTMANIPULATION_CONFIGURATION_INTERACTION
                    | DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_X
                    | DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_Y
                    | DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_INERTIA
                    | DIRECTMANIPULATION_CONFIGURATION_RAILS_X
                    | DIRECTMANIPULATION_CONFIGURATION_RAILS_Y
                    | DIRECTMANIPULATION_CONFIGURATION_SCALING,
            )?;
            helper.viewport.SetViewportOptions(DIRECTMANIPULATION_VIEWPORT_OPTIONS_MANUALUPDATE)?;
            let mut rect = RECT::default();
            GetClientRect(window, &mut rect)?;
            helper.size.set(PhysicalSize::new(
                (rect.right - rect.left).max(1) as u32,
                (rect.bottom - rect.top).max(1) as u32,
            ));
            helper.viewport.SetViewportRect(&helper.rect())?;
            let handler: IDirectManipulationViewportEventHandler =
                ViewportEvents { gesture }.into();
            helper
                .handler_cookie
                .set(Some(helper.viewport.AddEventHandler(Some(window), &handler)?));
            helper.manager.Activate(window)?;
            helper.activated.set(true);
            helper.viewport.Enable()?;
            helper.update_manager.Update(None)?;
        }
        Ok(helper)
    }

    /// Claim only precision touchpads. Windows keeps promoting unclaimed input
    /// to the existing WM_MOUSEWHEEL/WM_MOUSEHWHEEL path.
    pub(super) fn set_contact(&self, pointer_id: u32) -> bool {
        if self.closed.get() {
            return false;
        }
        let mut pointer_type = Default::default();
        if unsafe { GetPointerType(pointer_id, &mut pointer_type) }.is_err()
            || pointer_type != PT_TOUCHPAD
        {
            return false;
        }
        if !self.start_timer() {
            self.close();
            return false;
        }
        let was_interacting = {
            let mut gesture = self.gesture.lock().unwrap();
            std::mem::replace(&mut gesture.interacting, true)
        };
        if unsafe { self.viewport.SetContact(pointer_id) }.is_err() || self.closed.get() {
            if !self.closed.get() {
                self.gesture.lock().unwrap().interacting = was_interacting;
            }
            self.sync_timer();
            return false;
        }
        self.update();
        !self.closed.get()
    }

    pub(super) fn update(&self) {
        if self.closed.get() || self.updating.replace(true) {
            return;
        }
        let result = unsafe { self.update_manager.Update(None) };
        if result.is_err() {
            self.close();
        } else if !self.closed.get() {
            let reset = self.gesture.lock().unwrap().reset_pending;
            if reset {
                self.reset_transform();
            }
            self.sync_timer();
        }
        self.updating.set(false);
    }

    pub(super) fn resize(&self, size: PhysicalSize<u32>) {
        let size = PhysicalSize::new(size.width.max(1), size.height.max(1));
        if self.closed.get() || size == self.size.get() {
            return;
        }
        self.cancel();
        if self.closed.get() {
            return;
        }
        self.size.set(size);
        if unsafe { self.viewport.SetViewportRect(&self.rect()) }.is_err() {
            self.close();
        } else if !self.closed.get() {
            self.reset_transform();
        }
    }

    pub(super) fn cancel(&self) {
        if self.closed.get() {
            return;
        }
        {
            let mut gesture = self.gesture.lock().unwrap();
            if !gesture.interacting && !gesture.moving && !gesture.needs_reset() {
                return;
            }
            gesture.accepting = false;
            gesture.finish(TouchPhase::Cancelled);
            gesture.interacting = false;
            gesture.moving = false;
        }
        self.stop_timer();
        unsafe {
            let _ = self.viewport.Stop();
            let _ = self.viewport.ReleaseAllContacts();
        }
        if !self.closed.get() {
            self.reset_transform();
            self.gesture.lock().unwrap().accepting = !self.closed.get();
        }
    }

    pub(super) fn take_events(&self) -> Vec<WindowEvent> {
        std::mem::take(&mut self.gesture.lock().unwrap().events)
    }

    /// Detach before Destroyed is delivered; COM teardown can dispatch window
    /// messages, so repeated calls must be harmless.
    pub(super) fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        {
            let mut gesture = self.gesture.lock().unwrap();
            gesture.accepting = false;
            gesture.interacting = false;
            gesture.moving = false;
            gesture.finish(TouchPhase::Cancelled);
        }
        self.stop_timer();
        unsafe {
            if let Some(cookie) = self.handler_cookie.take() {
                let _ = self.viewport.RemoveEventHandler(cookie);
            }
            let _ = self.viewport.Stop();
            let _ = self.viewport.Abandon();
            if self.activated.replace(false) {
                let _ = self.manager.Deactivate(self.window);
            }
        }
    }

    fn rect(&self) -> RECT {
        let size = self.size.get();
        RECT { left: 0, top: 0, right: size.width as i32, bottom: size.height as i32 }
    }

    fn reset_transform(&self) {
        self.gesture.lock().unwrap().resetting = true;
        let size = self.size.get();
        let result = unsafe {
            self.viewport.ZoomToRect(0.0, 0.0, size.width as f32, size.height as f32, false)
        };
        if result.is_err() {
            self.close();
            return;
        }
        if !self.closed.get() && unsafe { self.update_manager.Update(None) }.is_err() {
            self.close();
        }
        let mut gesture = self.gesture.lock().unwrap();
        gesture.offset = PhysicalPosition::new(0.0, 0.0);
        gesture.scale = 1.0;
        gesture.resetting = false;
        gesture.reset_pending = false;
    }

    fn start_timer(&self) -> bool {
        if self.timer_running.get() {
            return true;
        }
        // WM_TIMER keeps native updates on the UI thread, including while the
        // application is waiting and while Windows supplies inertia.
        let timer = unsafe { SetTimer(Some(self.window), UPDATE_TIMER_ID, 10, None) };
        self.timer_running.set(timer != 0);
        timer != 0
    }

    fn stop_timer(&self) {
        if self.timer_running.replace(false) {
            let _ = unsafe { KillTimer(Some(self.window), UPDATE_TIMER_ID) };
        }
    }

    fn sync_timer(&self) {
        let gesture = self.gesture.lock().unwrap();
        let active = gesture.interacting || gesture.moving;
        drop(gesture);
        if self.closed.get() || !active {
            self.stop_timer();
        } else if !self.start_timer() {
            self.close();
        }
    }
}

impl Drop for DirectManipulation {
    fn drop(&mut self) {
        self.close();
    }
}

struct Apartment;

impl Apartment {
    fn new() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct Gesture {
    offset: PhysicalPosition<f64>,
    scale: f32,
    kind: GestureKind,
    interacting: bool,
    moving: bool,
    accepting: bool,
    resetting: bool,
    reset_pending: bool,
    events: Vec<WindowEvent>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GestureKind {
    None,
    Scroll,
    Pinch,
}

impl Default for Gesture {
    fn default() -> Self {
        Self {
            offset: PhysicalPosition::new(0.0, 0.0),
            scale: 1.0,
            kind: GestureKind::None,
            interacting: false,
            moving: false,
            accepting: true,
            resetting: false,
            reset_pending: false,
            events: Vec::new(),
        }
    }
}

impl Gesture {
    fn content_updated(&mut self, scale: f32, x: f32, y: f32) {
        if !self.accepting
            || self.resetting
            || !x.is_finite()
            || !y.is_finite()
            || !scale.is_finite()
            || scale <= 0.0
        {
            return;
        }
        let offset = PhysicalPosition::new(f64::from(x), f64::from(y));
        let delta = PhysicalPosition::new(offset.x - self.offset.x, offset.y - self.offset.y);
        let magnification = f64::from(scale / self.scale) - 1.0;
        self.offset = offset;
        self.scale = scale;

        // Native pinch transforms also translate. Once Windows recognizes a
        // pinch, omit that translation until the gesture finishes.
        if self.kind == GestureKind::Pinch || (scale - 1.0).abs() > 0.00001 {
            let phase = if self.kind == GestureKind::Pinch {
                TouchPhase::Moved
            } else {
                self.finish(TouchPhase::Ended);
                self.kind = GestureKind::Pinch;
                TouchPhase::Started
            };
            if magnification != 0.0 || phase == TouchPhase::Started {
                self.events.push(WindowEvent::PinchGesture {
                    device_id: DEVICE_ID,
                    delta: magnification,
                    phase,
                });
            }
            return;
        }
        if delta.x != 0.0 || delta.y != 0.0 {
            let phase = if self.kind == GestureKind::Scroll {
                TouchPhase::Moved
            } else {
                TouchPhase::Started
            };
            self.kind = GestureKind::Scroll;
            self.events.push(WindowEvent::MouseWheel {
                device_id: DEVICE_ID,
                delta: MouseScrollDelta::PixelDelta(delta),
                phase: phase.into(),
            });
        }
    }

    fn finish(&mut self, phase: TouchPhase) {
        match std::mem::replace(&mut self.kind, GestureKind::None) {
            GestureKind::Scroll => self.events.push(WindowEvent::MouseWheel {
                device_id: DEVICE_ID,
                delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.0)),
                phase: phase.into(),
            }),
            GestureKind::Pinch => self.events.push(WindowEvent::PinchGesture {
                device_id: DEVICE_ID,
                delta: 0.0,
                phase,
            }),
            GestureKind::None => {},
        }
    }

    fn needs_reset(&self) -> bool {
        self.offset.x != 0.0 || self.offset.y != 0.0 || self.scale != 1.0
    }
}

#[implement(IDirectManipulationViewportEventHandler, IDirectManipulationInteractionEventHandler)]
struct ViewportEvents {
    gesture: Arc<Mutex<Gesture>>,
}

impl IDirectManipulationViewportEventHandler_Impl for ViewportEvents_Impl {
    fn OnViewportStatusChanged(
        &self,
        _viewport: Ref<'_, IDirectManipulationViewport>,
        current: DIRECTMANIPULATION_STATUS,
        previous: DIRECTMANIPULATION_STATUS,
    ) -> Result<()> {
        let mut gesture = self.gesture.lock().unwrap();
        if !gesture.accepting || gesture.resetting {
            return Ok(());
        }
        if current == DIRECTMANIPULATION_RUNNING {
            if previous == DIRECTMANIPULATION_INERTIA {
                gesture.finish(TouchPhase::Ended);
            }
            gesture.moving = true;
        } else if current == DIRECTMANIPULATION_INERTIA {
            gesture.moving = true;
        } else if current == DIRECTMANIPULATION_READY {
            gesture.finish(TouchPhase::Ended);
            gesture.moving = false;
            gesture.reset_pending = gesture.needs_reset();
        } else if current == DIRECTMANIPULATION_DISABLED || current == DIRECTMANIPULATION_SUSPENDED
        {
            gesture.finish(TouchPhase::Cancelled);
            gesture.interacting = false;
            gesture.moving = false;
            gesture.reset_pending = gesture.needs_reset();
        }
        Ok(())
    }

    fn OnViewportUpdated(&self, _viewport: Ref<'_, IDirectManipulationViewport>) -> Result<()> {
        Ok(())
    }

    fn OnContentUpdated(
        &self,
        _viewport: Ref<'_, IDirectManipulationViewport>,
        content: Ref<'_, IDirectManipulationContent>,
    ) -> Result<()> {
        let mut transform = [0.0; 6];
        unsafe { content.ok()?.GetContentTransform(&mut transform)? };
        self.gesture.lock().unwrap().content_updated(transform[0], transform[4], transform[5]);
        Ok(())
    }
}

impl IDirectManipulationInteractionEventHandler_Impl for ViewportEvents_Impl {
    fn OnInteraction(
        &self,
        _viewport: Ref<'_, IDirectManipulationViewport2>,
        interaction: DIRECTMANIPULATION_INTERACTION_TYPE,
    ) -> Result<()> {
        let mut gesture = self.gesture.lock().unwrap();
        if interaction == DIRECTMANIPULATION_INTERACTION_END {
            gesture.interacting = false;
        } else if gesture.accepting
            && !gesture.resetting
            && interaction == DIRECTMANIPULATION_INTERACTION_BEGIN
        {
            gesture.interacting = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(x: f64, y: f64, phase: TouchPhase) -> WindowEvent {
        WindowEvent::MouseWheel {
            device_id: DEVICE_ID,
            delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(x, y)),
            phase: phase.into(),
        }
    }

    #[test]
    fn native_transforms_keep_both_axes_and_fractions_through_inertia() {
        let gesture = Arc::new(Mutex::new(Gesture::default()));
        let handler: IDirectManipulationViewportEventHandler =
            ViewportEvents { gesture: gesture.clone() }.into();
        {
            let mut gesture = gesture.lock().unwrap();
            gesture.content_updated(1.0, 0.0, 0.0);
            gesture.content_updated(1.0, 0.25, -0.5);
            gesture.content_updated(1.0, 1.0, -2.0);
        }
        unsafe {
            handler
                .OnViewportStatusChanged(
                    None,
                    DIRECTMANIPULATION_INERTIA,
                    DIRECTMANIPULATION_RUNNING,
                )
                .unwrap();
        }
        gesture.lock().unwrap().content_updated(1.0, 1.5, -3.0);
        unsafe {
            handler
                .OnViewportStatusChanged(None, DIRECTMANIPULATION_READY, DIRECTMANIPULATION_INERTIA)
                .unwrap();
        }
        let mut gesture = gesture.lock().unwrap();
        assert_eq!(
            gesture.events,
            vec![
                wheel(0.25, -0.5, TouchPhase::Started),
                wheel(0.75, -1.5, TouchPhase::Moved),
                wheel(0.5, -1.0, TouchPhase::Moved),
                wheel(0.0, 0.0, TouchPhase::Ended),
            ]
        );
        assert!(gesture.reset_pending);
        assert!(!gesture.moving);
        gesture.resetting = true;
        gesture.content_updated(1.0, 0.0, 0.0);
        assert_eq!(gesture.events.len(), 4, "resetting the native transform must not scroll back");
    }

    #[test]
    fn pinch_ends_pan_and_ignores_native_translation_noise() {
        let mut gesture = Gesture::default();
        gesture.content_updated(1.0, 0.0, -4.0);
        gesture.content_updated(1.25, 20.0, 10.0);
        gesture.content_updated(1.0, 30.0, 20.0);
        gesture.finish(TouchPhase::Cancelled);
        gesture.finish(TouchPhase::Cancelled);
        assert_eq!(
            gesture.events,
            vec![
                wheel(0.0, -4.0, TouchPhase::Started),
                wheel(0.0, 0.0, TouchPhase::Ended),
                WindowEvent::PinchGesture {
                    device_id: DEVICE_ID,
                    delta: 0.25,
                    phase: TouchPhase::Started
                },
                WindowEvent::PinchGesture {
                    device_id: DEVICE_ID,
                    delta: f64::from(1.0_f32 / 1.25) - 1.0,
                    phase: TouchPhase::Moved,
                },
                WindowEvent::PinchGesture {
                    device_id: DEVICE_ID,
                    delta: 0.0,
                    phase: TouchPhase::Cancelled
                },
            ]
        );
    }

    #[test]
    fn hidden_window_initializes_resizes_and_releases_native_touchpad_input() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPED,
        };

        struct HiddenWindow(HWND);
        impl Drop for HiddenWindow {
            fn drop(&mut self) {
                unsafe { DestroyWindow(self.0) }.unwrap();
            }
        }
        let window = HiddenWindow(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("winit touchpad test"),
                WS_OVERLAPPED,
                0,
                0,
                320,
                240,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        });
        let touchpad = DirectManipulation::new(window.0 .0).unwrap();
        assert_ne!(unsafe { touchpad.viewport.GetStatus() }.unwrap(), DIRECTMANIPULATION_DISABLED);
        assert!(!touchpad.set_contact(0), "an invalid contact must retain the wheel fallback");
        assert!(!touchpad.timer_running.get());

        touchpad.resize(PhysicalSize::new(480, 320));
        let rect = unsafe { touchpad.viewport.GetViewportRect() }.unwrap();
        assert_eq!((rect.right, rect.bottom), (480, 320));
        assert!(touchpad.take_events().is_empty());

        // Cancellation must stop the timer even before any nonzero transform.
        touchpad.gesture.lock().unwrap().interacting = true;
        assert!(touchpad.start_timer());
        touchpad.cancel();
        assert!(!touchpad.timer_running.get());
        assert!(touchpad.take_events().is_empty());

        touchpad.gesture.lock().unwrap().content_updated(1.0, 0.5, -1.25);
        touchpad.close();
        touchpad.close();
        assert_eq!(
            touchpad.take_events(),
            vec![wheel(0.5, -1.25, TouchPhase::Started), wheel(0.0, 0.0, TouchPhase::Cancelled),]
        );
        assert!(!touchpad.timer_running.get());
        assert!(!touchpad.set_contact(0));
    }
}
