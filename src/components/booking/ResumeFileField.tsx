"use client";

import { useRef, type DragEvent, type KeyboardEvent } from "react";

const ACCEPT =
  ".pdf,.doc,.docx,application/pdf,application/msword,application/vnd.openxmlformats-officedocument.wordprocessingml.document";

export const MAX_RESUME_BYTES = 5 * 1024 * 1024;

type Props = {
  id: string;
  file: File | null;
  onChange: (file: File | null) => void;
  onTooBig?: (file: File) => void;
};

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function ResumeFileField({ id, file, onChange, onTooBig }: Props) {
  const inputRef = useRef<HTMLInputElement>(null);

  function pick() {
    inputRef.current?.click();
  }

  function takeFile(next: File | null) {
    if (next && next.size > MAX_RESUME_BYTES) {
      if (inputRef.current) inputRef.current.value = "";
      onTooBig?.(next);
      return;
    }
    onChange(next);
  }

  function onDrop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault();
    const next = event.dataTransfer.files?.[0];
    if (next) takeFile(next);
  }

  function onKey(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      pick();
    }
  }

  return (
    <div className="pca-field">
      <label className="req" htmlFor={id}>
        Resume
      </label>
      <input
        ref={inputRef}
        id={id}
        className="pca-resume-input"
        type="file"
        accept={ACCEPT}
        onChange={(e) => takeFile(e.target.files?.[0] || null)}
      />
      <div
        className={`pca-resume-drop${file ? " has-file" : ""}`}
        role="button"
        tabIndex={0}
        onClick={pick}
        onKeyDown={onKey}
        onDragOver={(e) => e.preventDefault()}
        onDrop={onDrop}
      >
        <span className="pca-resume-icon" aria-hidden>
          {file ? (
            <svg viewBox="0 0 24 24" fill="none">
              <path
                d="M7 3.5h7.2L19 8.4V20a1.5 1.5 0 0 1-1.5 1.5h-10A1.5 1.5 0 0 1 6 20V5A1.5 1.5 0 0 1 7.5 3.5H7z"
                stroke="currentColor"
                strokeWidth="1.6"
              />
              <path d="M14 3.5V8h5" stroke="currentColor" strokeWidth="1.6" />
            </svg>
          ) : (
            <svg viewBox="0 0 24 24" fill="none">
              <path
                d="M12 15V7m0 0l-3.2 3.2M12 7l3.2 3.2"
                stroke="currentColor"
                strokeWidth="1.7"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              <path
                d="M6 16.5v2A1.5 1.5 0 0 0 7.5 20h9a1.5 1.5 0 0 0 1.5-1.5v-2"
                stroke="currentColor"
                strokeWidth="1.7"
                strokeLinecap="round"
              />
            </svg>
          )}
        </span>
        <span className="pca-resume-meta">
          {file ? (
            <>
              <strong>{file.name}</strong>
              <span>{formatSize(file.size)} · PDF or Word</span>
            </>
          ) : (
            <>
              <strong>Drop resume here or click to upload</strong>
              <span>PDF or Word · max 5 MB</span>
            </>
          )}
        </span>
        {file ? (
          <button
            type="button"
            className="pca-resume-clear"
            onClick={(e) => {
              e.stopPropagation();
              onChange(null);
              if (inputRef.current) inputRef.current.value = "";
            }}
          >
            Remove
          </button>
        ) : null}
      </div>
    </div>
  );
}
