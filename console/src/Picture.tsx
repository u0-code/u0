// Giving a ghost a picture: choose an image, frame her face in it, keep the
// square. The original never leaves this window; what is saved is the crop.

import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "./Icon";

const VIEW = 264; // the framing square on screen
const OUT = 512; // the saved square
const MAX_ZOOM = 6;

type Frame = { x: number; y: number; zoom: number };

/** Read a chosen or dropped file as a data URL, if it is a picture. */
export function readPicture(file: File | undefined): Promise<string | null> {
  return new Promise((resolve) => {
    if (!file || !file.type.startsWith("image/")) return resolve(null);
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : null);
    reader.onerror = () => resolve(null);
    reader.readAsDataURL(file);
  });
}

/** Open the system's file chooser for one picture. */
export function choosePicture(): Promise<string | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/png,image/jpeg,image/webp";
    input.onchange = () => void readPicture(input.files?.[0]).then(resolve);
    input.oncancel = () => resolve(null);
    input.click();
  });
}

/**
 * Everything a view needs to let someone give a ghost a picture: a way to
 * choose one, a way to take a dropped one, and the framing sheet to render.
 */
export function usePicture(onDone: (picture: string) => void) {
  const [source, setSource] = useState<string | null>(null);
  const take = (picture: string | null) => {
    if (picture) setSource(picture);
  };
  return {
    pick: () => void choosePicture().then(take),
    offer: (file: File | undefined) => void readPicture(file).then(take),
    framer: source ? (
      <Framer
        source={source}
        onCancel={() => setSource(null)}
        onDone={(picture) => {
          setSource(null);
          onDone(picture);
        }}
      />
    ) : null,
  };
}

function Framer({
  source,
  onDone,
  onCancel,
}: {
  source: string;
  onDone: (picture: string) => void;
  onCancel: () => void;
}) {
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  const [frame, setFrame] = useState<Frame>({ x: 0, y: 0, zoom: 1 });
  const drag = useRef<{ x: number; y: number; from: Frame } | null>(null);

  useEffect(() => {
    const loading = new Image();
    loading.onload = () => {
      setImage(loading);
      // Faces are usually near the top of a picture of a person.
      const fit = VIEW / Math.min(loading.width, loading.height);
      setFrame({ x: (VIEW - loading.width * fit) / 2, y: 0, zoom: 1 });
    };
    loading.src = source;
  }, [source]);

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [onCancel]);

  const fit = image ? VIEW / Math.min(image.width, image.height) : 1;
  const scale = fit * frame.zoom;

  const clamp = useCallback(
    (next: Frame): Frame => {
      if (!image) return next;
      const zoom = Math.min(MAX_ZOOM, Math.max(1, next.zoom));
      const w = image.width * fit * zoom;
      const h = image.height * fit * zoom;
      return { zoom, x: Math.min(0, Math.max(VIEW - w, next.x)), y: Math.min(0, Math.max(VIEW - h, next.y)) };
    },
    [image, fit],
  );

  /** Zoom about a point of the square, so what is under it stays under it. */
  const zoomTo = (zoom: number, cx = VIEW / 2, cy = VIEW / 2) => {
    setFrame((was) => {
      const to = Math.min(MAX_ZOOM, Math.max(1, zoom));
      const k = to / was.zoom;
      return clamp({ zoom: to, x: cx - (cx - was.x) * k, y: cy - (cy - was.y) * k });
    });
  };

  const save = () => {
    if (!image) return;
    const canvas = document.createElement("canvas");
    canvas.width = OUT;
    canvas.height = OUT;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.fillStyle = "#1e1e1e";
    ctx.fillRect(0, 0, OUT, OUT);
    ctx.imageSmoothingQuality = "high";
    const side = VIEW / scale;
    ctx.drawImage(image, -frame.x / scale, -frame.y / scale, side, side, 0, 0, OUT, OUT);
    onDone(canvas.toDataURL("image/jpeg", 0.92));
  };

  return (
    <div className="veil" onPointerDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className="sheet framer" role="dialog" aria-label="Frame her picture">
        <header>
          <b>Frame the picture</b>
          <button className="ib" onClick={onCancel} aria-label="Cancel">
            <Icon name="close" />
          </button>
        </header>
        <div
          className="frame"
          style={{ width: VIEW, height: VIEW }}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            drag.current = { x: e.clientX, y: e.clientY, from: frame };
          }}
          onPointerMove={(e) => {
            const d = drag.current;
            if (d) setFrame(clamp({ ...d.from, x: d.from.x + e.clientX - d.x, y: d.from.y + e.clientY - d.y }));
          }}
          onPointerUp={() => (drag.current = null)}
          onWheel={(e) => {
            const box = e.currentTarget.getBoundingClientRect();
            zoomTo(frame.zoom * Math.exp(-e.deltaY / 300), e.clientX - box.left, e.clientY - box.top);
          }}
        >
          {image && (
            <img
              src={source}
              alt=""
              draggable={false}
              style={{
                width: image.width * scale,
                height: image.height * scale,
                transform: `translate(${frame.x}px, ${frame.y}px)`,
              }}
            />
          )}
          <i className="frame-mask" />
        </div>
        <label className="zoom">
          <Icon name="image" small />
          <input
            type="range"
            min={1}
            max={MAX_ZOOM}
            step={0.01}
            value={frame.zoom}
            onChange={(e) => zoomTo(Number(e.target.value))}
            aria-label="Zoom"
          />
          <Icon name="image" />
        </label>
        <footer>
          <span>Drag to move. Scroll or slide to zoom.</span>
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn primary" onClick={save} disabled={!image}>
            Use picture
          </button>
        </footer>
      </div>
    </div>
  );
}
