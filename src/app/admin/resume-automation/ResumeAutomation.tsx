"use client";

import { Skel } from "@/components/ui/Skeleton";
import { useEffect, useState } from "react";
import { OfficeEditor } from "@/components/office/OfficeEditor";

/**
 * Resume Maker — nothing but the ONLYOFFICE editor, filling the whole tab.
 *
 * The AI drafting UI is not here: it is an ONLYOFFICE plugin docked in the
 * editor's own left panel, alongside Comments and Headings, registered per
 * session via editorConfig.plugins (see backend/src/plugin.rs).
 */
export function ResumeAutomation() {
  const [doc, setDoc] = useState<
    { url?: string; template?: string; title?: string } | null
  >(null);
  /** Opened without ?template= or ?doc= — there is nothing to edit. */
  const [nothingChosen, setNothingChosen] = useState(false);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);

    // Template ids are resolved server-side: it mints a signed download URL the
    // Document Server can reach, so the browser never has to know how to get
    // back to us from inside Docker.
    const template = (params.get("template") || "").trim();
    if (template) {
      // No title: the server knows the template's real filename, and a made-up
      // one without an extension is what produced fileType "template".
      setDoc({ template });
      return;
    }

    const raw = (params.get("doc") || "").trim();
    if (!raw) {
      setNothingChosen(true);
      return;
    }

    // The Document Server fetches the file itself and is not in the browser's
    // network position — "localhost" would mean the container, not this machine
    // — so same-origin paths are rewritten to an address it can reach.
    const url = /^https?:\/\//i.test(raw)
      ? raw
      : (() => {
          const local =
            window.location.hostname === "localhost" || window.location.hostname === "127.0.0.1";
          const origin = local
            ? `http://host.docker.internal:${window.location.port || "3000"}`
            : window.location.origin;
          return `${origin}${raw.startsWith("/") ? "" : "/"}${raw}`;
        })();

    const name = decodeURIComponent(url.split("?")[0].split("/").pop() || "document.docx");
    setDoc({ url, title: /\.\w{2,5}$/.test(name) ? name : `${name}.docx` });
  }, []);

  return (
    <div className="office-full">
      {doc ? (
        <OfficeEditor
          key={doc.template || doc.url}
          url={doc.url}
          template={doc.template}
          title={doc.title}
          mode="edit"
        />
      ) : nothingChosen ? (
        <div className="office-loading" role="status">
          <p>
            Pick a template on the <a href="/admin">Admin page</a> to open it here.
          </p>
        </div>
      ) : (
        <div aria-busy="true" aria-label="Opening editor" style={{ padding: 24, display: "grid", gap: 14 }}>
          <Skel h={40} r={8} />
          <div style={{ display: "grid", gridTemplateColumns: "1fr", justifyItems: "center", gap: 10, paddingTop: 12 }}>
            <Skel w="min(780px, 92%)" h="70vh" r={4} />
          </div>
        </div>
      )}
    </div>
  );
}
