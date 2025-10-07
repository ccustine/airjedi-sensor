//! WebSocket output module for real-time ADS-B data streaming to web applications
//!
//! This module provides a WebSocket server that broadcasts SBS-1 format messages to web clients.
//! It enables real-time streaming of ADS-B data to web applications with automatic client
//! connection management and message buffering.
//!
//! ## Usage
//! Web clients can connect to the WebSocket server and receive real-time SBS-1 CSV messages:
//! ```javascript
//! const ws = new WebSocket('ws://localhost:30008/adsb');
//! ws.onmessage = function(event) {
//!     // event.data contains SBS-1 CSV format text
//!     const sbs1Data = event.data;
//!     // Parse CSV: MSG,type,session,aircraft,icao,flight,date,time,...
//! };
//! ```
//!
//! ## Message Format
//! Messages are delivered in SBS-1/BaseStation CSV format:
//! - MSG,1: Aircraft identification (callsign)
//! - MSG,3: Airborne position (lat, lon, altitude)
//! - MSG,4: Airborne velocity (speed, heading, vertical rate)

use crate::sbs1_output::Sbs1Message;
use crate::{AdsbIcao, AircraftRecord};
use anyhow::Result;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use tracing::{debug, error, info, warn};

/// WebSocket message containing SBS-1 format data
#[derive(Debug, Clone)]
pub struct WebSocketMessage {
    pub sbs1_data: String,
}

impl WebSocketMessage {
    /// Create a WebSocket message from SBS-1 message
    pub fn from_sbs1_message(sbs1_msg: &Sbs1Message) -> Self {
        Self {
            sbs1_data: sbs1_msg.encode(),
        }
    }
}

/// WebSocket server for streaming ADS-B data
pub struct WebSocketServer {
    listener: TcpListener,
    receiver: broadcast::Receiver<WebSocketMessage>,
}

impl WebSocketServer {
    /// Create a new WebSocket server listening on the specified port
    pub async fn new(port: u16, receiver: broadcast::Receiver<WebSocketMessage>) -> Result<Self> {
        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        info!("WebSocket ADS-B server listening on {}", addr);

        Ok(Self {
            listener,
            receiver,
        })
    }

    /// Run the WebSocket server, accepting connections and streaming data
    pub async fn run(self) -> Result<()> {
        // Accept new WebSocket connections
        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    info!("WebSocket client connecting from {}", addr);
                    let message_receiver = self.receiver.resubscribe();

                    tokio::spawn(async move {
                        match Self::handle_websocket_connection(stream, message_receiver).await {
                            Ok(_) => {
                                info!("WebSocket client {} disconnected gracefully", addr);
                            }
                            Err(e) => {
                                debug!("WebSocket client {} disconnected: {}", addr, e);
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept WebSocket connection: {}", e);
                }
            }
        }
    }

    /// Handle a single WebSocket client connection
    async fn handle_websocket_connection(
        stream: TcpStream,
        mut message_receiver: broadcast::Receiver<WebSocketMessage>,
    ) -> Result<()> {
        let ws_stream = accept_async(stream).await?;
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        info!("WebSocket client connected successfully");

        // Spawn task to handle incoming WebSocket messages (ping/pong, close, etc.)
        let mut ping_task = tokio::spawn(async move {
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(Message::Ping(_payload)) => {
                        // Respond to ping with pong - but we can't send from here
                        debug!("Received ping from WebSocket client");
                    }
                    Ok(Message::Close(_)) => {
                        debug!("WebSocket client sent close frame");
                        break;
                    }
                    Err(_) => {
                        debug!("WebSocket client connection error");
                        break;
                    }
                    _ => {
                        // Ignore other message types
                    }
                }
            }
        });

        // Main message sending loop
        loop {
            tokio::select! {
                // Handle broadcast messages
                msg = message_receiver.recv() => {
                    match msg {
                        Ok(message) => {
                            let text_msg = Message::Text(message.sbs1_data);
                            if let Err(e) = ws_sender.send(text_msg).await {
                                debug!("Failed to send WebSocket message: {}", e);
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!("WebSocket client lagged, skipped {} messages", skipped);
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            debug!("WebSocket message channel closed");
                            break;
                        }
                    }
                }
                // Handle connection monitoring
                _ = &mut ping_task => {
                    debug!("WebSocket client connection monitoring task finished");
                    break;
                }
            }
        }

        Ok(())
    }
}

/// WebSocket message broadcaster
pub struct WebSocketBroadcaster {
    sender: broadcast::Sender<WebSocketMessage>,
}

impl WebSocketBroadcaster {
    /// Create a new WebSocket broadcaster with the specified channel capacity
    pub fn new(capacity: usize) -> (Self, broadcast::Receiver<WebSocketMessage>) {
        let (sender, receiver) = broadcast::channel(capacity);
        (Self { sender }, receiver)
    }

    /// Broadcast an SBS-1 message to WebSocket clients
    pub fn broadcast_message(&self, sbs1_msg: Sbs1Message) -> Result<()> {
        let message = WebSocketMessage::from_sbs1_message(&sbs1_msg);
        match self.sender.send(message) {
            Ok(receiver_count) => {
                debug!("Broadcasted WebSocket message to {} clients", receiver_count);
                Ok(())
            }
            Err(_) => {
                // No receivers, which is fine
                Ok(())
            }
        }
    }

    /// Get the number of active WebSocket clients
    pub fn client_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// WebSocket output module implementing the OutputModule trait
pub struct WebSocketOutput {
    name: String,
    port: u16,
    broadcaster: WebSocketBroadcaster,
    is_running: bool,
}

impl WebSocketOutput {
    /// Create a new WebSocket output module
    pub async fn new(config: crate::output_module::OutputModuleConfig) -> Result<Self> {
        let (broadcaster, receiver) = WebSocketBroadcaster::new(config.buffer_capacity);
        
        // Start the WebSocket server
        let server = WebSocketServer::new(config.port, receiver).await?;
        tokio::spawn(async move {
            if let Err(e) = server.run().await {
                error!("WebSocket server error: {}", e);
            }
        });

        Ok(Self {
            name: config.name,
            port: config.port,
            broadcaster,
            is_running: true,
        })
    }
}

// Implement the base trait for common functionality
impl crate::output_module::OutputModuleBase for WebSocketOutput {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "WebSocket server for real-time SBS-1 format ADS-B data streaming to web applications"
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn client_count(&self) -> usize {
        self.broadcaster.client_count()
    }

    fn is_running(&self) -> bool {
        self.is_running
    }

    fn stop(&mut self) -> Result<()> {
        self.is_running = false;
        Ok(())
    }
}

// Implement the state output trait for broadcasting aircraft state
impl crate::output_module::StateOutputModule for WebSocketOutput {
    fn broadcast_aircraft_update(&self, icao: &AdsbIcao, record: &AircraftRecord) -> Result<()> {
        let icao_str = format!("{:02X}{:02X}{:02X}", icao.0[0], icao.0[1], icao.0[2]);

        // Broadcast identification message if we have a callsign
        if let Some(ref callsign) = record.callsign {
            let msg = Sbs1Message::identification(&icao_str, callsign, record.last_seen);
            self.broadcaster.broadcast_message(msg)?;
        }

        // Broadcast position message if we have position data
        if let Some(pos_record) = record.positions.last() {
            let msg = Sbs1Message::airborne_position(
                &icao_str,
                pos_record.position.latitude,
                pos_record.position.longitude,
                pos_record.position.altitude,
                pos_record.time,
            );
            self.broadcaster.broadcast_message(msg)?;
        }

        // Broadcast velocity message if we have velocity data
        if let Some(vel_record) = record.velocities.last() {
            let msg = Sbs1Message::airborne_velocity(
                &icao_str,
                vel_record.velocity.ground_speed,
                vel_record.velocity.heading,
                vel_record.velocity.vertical_rate,
                vel_record.time,
            );
            self.broadcaster.broadcast_message(msg)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn test_websocket_message_from_sbs1() {
        let sbs1_msg = Sbs1Message::identification("A12345", "TEST123", SystemTime::now());

        let ws_message = WebSocketMessage::from_sbs1_message(&sbs1_msg);
        assert!(!ws_message.sbs1_data.is_empty());
        assert!(ws_message.sbs1_data.starts_with("MSG,1,"));
        assert!(ws_message.sbs1_data.contains("A12345"));
        assert!(ws_message.sbs1_data.contains("TEST123"));
    }

    #[test]
    fn test_websocket_message_format() {
        let sbs1_msg = Sbs1Message::airborne_position(
            "ABCDEF",
            37.5,
            -122.3,
            Some(35000),
            SystemTime::now(),
        );

        let ws_message = WebSocketMessage::from_sbs1_message(&sbs1_msg);
        assert!(ws_message.sbs1_data.starts_with("MSG,3,"));
        assert!(ws_message.sbs1_data.contains("ABCDEF"));
        assert!(ws_message.sbs1_data.contains("37.5"));
        assert!(ws_message.sbs1_data.contains("-122.3"));
    }
}