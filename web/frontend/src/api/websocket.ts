import type { BrowserMessage, ServerMessage } from './types';

export type MessageHandler = (message: ServerMessage) => void;

const RECONNECT_DELAY_MS = 5000;

export class WebSocketClient {
  private ws: WebSocket | null = null;
  private url: string;
  private handlers: Set<MessageHandler> = new Set();
  private isConnecting = false;

  constructor(url?: string) {
    //
    // Use wss:// for HTTPS and ws:// for HTTP.
    //
    if (!url) {
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      url = `${protocol}//${window.location.host}/ws`;
    }
    this.url = url;
  }

  connect(): Promise<void> {
    if (this.isConnecting || (this.ws && this.ws.readyState === WebSocket.OPEN)) {
      return Promise.resolve();
    }

    this.isConnecting = true;

    return new Promise((resolve, reject) => {
      try {
        this.ws = new WebSocket(this.url);

        this.ws.onopen = () => {
          console.log('WebSocket connected');
          this.isConnecting = false;
          resolve();
        };

        this.ws.onmessage = (event) => {
          try {
            const message: ServerMessage = JSON.parse(event.data);
            this.handlers.forEach((handler) => handler(message));
          } catch (e) {
            console.error('Failed to parse WebSocket message:', e);
          }
        };

        this.ws.onerror = (error) => {
          console.error('WebSocket error:', error);
          this.isConnecting = false;
        };

        this.ws.onclose = () => {
          console.log('WebSocket disconnected');
          this.isConnecting = false;
          this.ws = null;
          this.attemptReconnect();
        };
      } catch (error) {
        this.isConnecting = false;
        reject(error);
      }
    });
  }

  private attemptReconnect(): void {
    console.log(`WebSocket reconnecting in ${RECONNECT_DELAY_MS / 1000} seconds...`);
    setTimeout(() => {
      this.connect().catch(console.error);
    }, RECONNECT_DELAY_MS);
  }

  disconnect(): void {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  send(message: BrowserMessage): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      const json = JSON.stringify(message);
      console.log('WebSocket sending:', json);
      this.ws.send(json);
    } else {
      console.error('WebSocket is not connected, readyState:', this.ws?.readyState);
    }
  }

  addHandler(handler: MessageHandler): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  get isConnected(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
  }
}

//
// Singleton instance.
//
export const wsClient = new WebSocketClient();
