// US-25 : export d'un sous-intervalle [start, end] du replay en vidéo webm. Approche retenue :
// enregistrer le canvas LIVE (`canvas.captureStream`) pendant une lecture réelle pilotée par le
// composant, PAS un rendu offscreen/accéléré — évite de toucher à la boucle de dessin existante
// et garde le enregistrement en phase avec `captureStream` (qui échantillonne le canvas au fil du
// temps réel, pas frame-par-frame sur demande). Module sans React : mécanique MediaRecorder pure,
// pilotée depuis Replay2D.tsx.

/** Nombre de frames pour un intervalle [start, end] à `fps` — pure, testée en unit.
 *  0 si l'intervalle est vide ou invalide (end <= start). */
export function clipFrames(start: number, end: number, fps: number): number {
  if (end <= start) return 0;
  return Math.round((end - start) * fps);
}

/** Feature-detection : MediaRecorder + captureStream ne sont pas garantis (vieux navigateurs,
 *  certains WebViews). Vérifié avant d'afficher/activer le bouton d'export. */
export function supportsClipExport(): boolean {
  return (
    typeof MediaRecorder !== "undefined" &&
    typeof HTMLCanvasElement !== "undefined" &&
    typeof HTMLCanvasElement.prototype.captureStream === "function"
  );
}

const CANDIDATE_MIME_TYPES = ["video/webm;codecs=vp9", "video/webm;codecs=vp8", "video/webm"];

function pickMimeType(): string | undefined {
  return CANDIDATE_MIME_TYPES.find((mt) => MediaRecorder.isTypeSupported(mt));
}

/** Démarre l'enregistrement du flux vidéo du canvas ; `stop()` arrête le recorder et résout avec
 *  le Blob complet (webm) une fois `onstop` déclenché — pas de perte du dernier chunk. */
export function recordCanvasStream(canvas: HTMLCanvasElement, fps = 30): { stop: () => Promise<Blob> } {
  const stream = canvas.captureStream(fps);
  const mimeType = pickMimeType();
  const recorder = mimeType ? new MediaRecorder(stream, { mimeType }) : new MediaRecorder(stream);
  const chunks: BlobPart[] = [];

  recorder.ondataavailable = (e: BlobEvent) => {
    if (e.data && e.data.size > 0) chunks.push(e.data);
  };

  recorder.start();

  const stop = (): Promise<Blob> =>
    new Promise((resolve) => {
      recorder.onstop = () => {
        resolve(new Blob(chunks, { type: mimeType ?? "video/webm" }));
      };
      recorder.stop();
    });

  return { stop };
}

/** Déclenche le téléchargement d'un Blob via un `<a download>` temporaire (jamais ajouté au DOM
 *  visible). Révoque l'URL objet aussitôt le clic synchrone effectué. */
export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}
