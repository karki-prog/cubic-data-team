/**
 * Sheet row appends for website booking (connector POC tabs + Phone calls tab).
 *
 * POC tabs: store raw URLs in hidden S:U, write =HYPERLINK on O:Q, and restore
 * every row after insertRowBefore — otherwise existing links flatten to labels.
 */

import { getSheets } from "@/lib/google/auth";
import { GOOGLE_CONFIG } from "@/lib/google/config";

const LINK_FG = { red: 0.067, green: 0.333, blue: 0.8 };
const CONNECTOR_LINK_LABELS = [
  "Resume Link",
  "Job Description Link",
  "Drive Folder Link",
] as const;
const CONNECTOR_VISIBLE_COLS = [15, 16, 17] as const;
const PHONE_CALLS_LINK_LABELS = [
  "Job Description Link",
  "Resume Link",
  "Drive Folder Link",
] as const;
const PHONE_CALLS_VISIBLE_COLS = [14, 15, 16] as const;
const PHONE_CALLS_URL_COLS = [20, 21, 22] as const;
const PHONE_CALLS_DATA_START_ROW = 2;

type RichTextLinkCell = {
  column1: number;
  url: string;
  label: string;
};

function hyperlinkFormula(url: string, label: string): string {
  const href = String(url || "").trim();
  const text = String(label || href).trim() || href;
  return `=HYPERLINK("${href.replace(/"/g, '""')}","${text.replace(/"/g, '""')}")`;
}

function hyperlinkFormulaUrl(formula: string): string {
  const text = String(formula || "").trim();
  if (!/^=HYPERLINK/i.test(text)) return "";
  const match = text.match(/HYPERLINK\s*\(\s*"([^"]+)"/i);
  return match ? match[1] : "";
}

/**
 * Write clickable rich-text links. USER_ENTERED =HYPERLINK(...) formulas
 * get flattened to plain labels on Phone calls / Client List tabs.
 */
async function writeRichTextHyperlinks(
  spreadsheetId: string,
  sheetId: number,
  row1: number,
  cells: RichTextLinkCell[]
) {
  const requests = cells
    .map((cell) => {
      const url = String(cell.url || "").trim();
      const label = String(cell.label || url).trim() || url;
      if (!url) return null;
      return {
        updateCells: {
          start: {
            sheetId,
            rowIndex: row1 - 1,
            columnIndex: cell.column1 - 1,
          },
          rows: [
            {
              values: [
                {
                  userEnteredValue: { stringValue: label },
                  textFormatRuns: [
                    {
                      startIndex: 0,
                      format: {
                        link: { uri: url },
                        foregroundColor: LINK_FG,
                        underline: true,
                      },
                    },
                  ],
                },
              ],
            },
          ],
          fields: "userEnteredValue,textFormatRuns",
        },
      };
    })
    .filter((request): request is NonNullable<typeof request> => Boolean(request));

  if (!requests.length) return;
  const sheets = await getSheets();
  await sheets.spreadsheets.batchUpdate({
    spreadsheetId,
    requestBody: { requests },
  });
}

async function writeConnectorHiddenUrls(
  spreadsheetId: string,
  sheetName: string,
  row1: number,
  resumeUrl: string,
  jdUrl: string,
  folderUrl: string
) {
  const sheets = await getSheets();
  await sheets.spreadsheets.values.update({
    spreadsheetId,
    range: `'${sheetName}'!S${row1}:U${row1}`,
    valueInputOption: "RAW",
    requestBody: {
      values: [[
        String(resumeUrl || "").trim(),
        String(jdUrl || "").trim(),
        String(folderUrl || "").trim(),
      ]],
    },
  });
}

async function restoreConnectorHyperlinks(
  spreadsheetId: string,
  sheetName: string,
  dataStartRow: number
): Promise<number> {
  const sheets = await getSheets();
  const colA = await sheets.spreadsheets.values.get({
    spreadsheetId,
    range: `'${sheetName}'!A:A`,
  });
  const lastRow = colA.data.values?.length ?? 0;
  if (lastRow < dataStartRow) return 0;

  const [formulasRes, hiddenRes] = await Promise.all([
    sheets.spreadsheets.values.get({
      spreadsheetId,
      range: `'${sheetName}'!O${dataStartRow}:Q${lastRow}`,
      valueRenderOption: "FORMULA",
    }),
    sheets.spreadsheets.values.get({
      spreadsheetId,
      range: `'${sheetName}'!S${dataStartRow}:U${lastRow}`,
    }),
  ]);

  const formulas = formulasRes.data.values ?? [];
  const hidden = hiddenRes.data.values ?? [];
  const n = lastRow - dataStartRow + 1;
  const sheetId = await getSheetId(spreadsheetId, sheetName);
  const requests: object[] = [];
  let restored = 0;

  for (let i = 0; i < n; i++) {
    const hiddenRow = hidden[i] ?? [];
    const formulaRow = formulas[i] ?? [];
    const row1 = dataStartRow + i;

    for (let c = 0; c < 3; c++) {
      const href = String(hiddenRow[c] || "").trim();
      if (!href.startsWith("http")) continue;
      const curFormula = String(formulaRow[c] || "").trim();
      const curUrl = hyperlinkFormulaUrl(curFormula);
      if (/^=HYPERLINK/i.test(curFormula) && curUrl === href) continue;

      requests.push({
        updateCells: {
          start: {
            sheetId,
            rowIndex: row1 - 1,
            columnIndex: CONNECTOR_VISIBLE_COLS[c] - 1,
          },
          rows: [
            {
              values: [
                {
                  userEnteredValue: {
                    formulaValue: hyperlinkFormula(href, CONNECTOR_LINK_LABELS[c]),
                  },
                },
              ],
            },
          ],
          fields: "userEnteredValue",
        },
      });
      restored++;
    }
  }

  for (let offset = 0; offset < requests.length; offset += 80) {
    await sheets.spreadsheets.batchUpdate({
      spreadsheetId,
      requestBody: { requests: requests.slice(offset, offset + 80) },
    });
  }

  return restored;
}

async function writePhoneCallsHiddenUrls(
  spreadsheetId: string,
  sheetName: string,
  row1: number,
  jdUrl: string,
  resumeUrl: string,
  folderUrl: string
) {
  const sheets = await getSheets();
  await sheets.spreadsheets.values.update({
    spreadsheetId,
    range: `'${sheetName}'!T${row1}:V${row1}`,
    valueInputOption: "RAW",
    requestBody: {
      values: [[
        String(jdUrl || "").trim(),
        String(resumeUrl || "").trim(),
        String(folderUrl || "").trim(),
      ]],
    },
  });
}

async function restorePhoneCallsHyperlinks(
  spreadsheetId: string,
  sheetName: string
): Promise<number> {
  const sheets = await getSheets();
  const colA = await sheets.spreadsheets.values.get({
    spreadsheetId,
    range: `'${sheetName}'!A:A`,
  });
  const lastRow = colA.data.values?.length ?? 0;
  const dataStartRow = PHONE_CALLS_DATA_START_ROW;
  if (lastRow < dataStartRow) return 0;

  const [formulasRes, hiddenRes] = await Promise.all([
    sheets.spreadsheets.values.get({
      spreadsheetId,
      range: `'${sheetName}'!N${dataStartRow}:P${lastRow}`,
      valueRenderOption: "FORMULA",
    }),
    sheets.spreadsheets.values.get({
      spreadsheetId,
      range: `'${sheetName}'!T${dataStartRow}:V${lastRow}`,
    }),
  ]);

  const formulas = formulasRes.data.values ?? [];
  const hidden = hiddenRes.data.values ?? [];
  const n = lastRow - dataStartRow + 1;
  const sheetId = await getSheetId(spreadsheetId, sheetName);
  const requests: object[] = [];
  let restored = 0;

  for (let i = 0; i < n; i++) {
    const hiddenRow = hidden[i] ?? [];
    const formulaRow = formulas[i] ?? [];
    const row1 = dataStartRow + i;

    for (let c = 0; c < 3; c++) {
      const href = String(hiddenRow[c] || "").trim();
      if (!href.startsWith("http")) continue;
      const curFormula = String(formulaRow[c] || "").trim();
      const curUrl = hyperlinkFormulaUrl(curFormula);
      if (/^=HYPERLINK/i.test(curFormula) && curUrl === href) continue;

      requests.push({
        updateCells: {
          start: {
            sheetId,
            rowIndex: row1 - 1,
            columnIndex: PHONE_CALLS_VISIBLE_COLS[c] - 1,
          },
          rows: [
            {
              values: [
                {
                  userEnteredValue: {
                    formulaValue: hyperlinkFormula(href, PHONE_CALLS_LINK_LABELS[c]),
                  },
                },
              ],
            },
          ],
          fields: "userEnteredValue",
        },
      });
      restored++;
    }
  }

  for (let offset = 0; offset < requests.length; offset += 80) {
    await sheets.spreadsheets.batchUpdate({
      spreadsheetId,
      requestBody: { requests: requests.slice(offset, offset + 80) },
    });
  }

  return restored;
}

function formatMeetingTime(time: string) {
  const text = String(time || "").trim();
  if (!text) return "";
  if (/am|pm/i.test(text)) {
    return /cst/i.test(text) ? text.replace(/\s+CST\s*$/i, " CST") : `${text} CST`;
  }
  return `${text} CST`;
}

function formatTimestamp(date = new Date()) {
  return date.toLocaleString("en-US", {
    timeZone: GOOGLE_CONFIG.timezone,
    month: "numeric",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  });
}

function resolvePocSheet(poc: string): string {
  const text = String(poc || "")
    .trim()
    .toLowerCase()
    .replace(/\s+/g, " ");
  for (const name of GOOGLE_CONFIG.pocSheets) {
    if (name.toLowerCase() === text) return name;
  }
  if (text.startsWith("pras")) return "Prasanna";
  if (text.startsWith("sajit")) return "Sajit";
  if (text.startsWith("saksham")) return "Saksham";
  return GOOGLE_CONFIG.pocSheets[0];
}

async function getSheetId(
  spreadsheetId: string,
  sheetName: string
): Promise<number> {
  const sheets = await getSheets();
  const meta = await sheets.spreadsheets.get({
    spreadsheetId,
    fields: "sheets.properties(sheetId,title)",
  });
  const sheet = meta.data.sheets?.find((s) => s.properties?.title === sheetName);
  const sheetId = sheet?.properties?.sheetId;
  if (sheetId == null) {
    throw new Error(`Sheet "${sheetName}" not found.`);
  }
  return sheetId;
}

async function insertRowAt(
  spreadsheetId: string,
  sheetName: string,
  rowIndex0: number
) {
  const sheets = await getSheets();
  const sheetId = await getSheetId(spreadsheetId, sheetName);
  await sheets.spreadsheets.batchUpdate({
    spreadsheetId,
    requestBody: {
      requests: [
        {
          insertDimension: {
            range: {
              sheetId,
              dimension: "ROWS",
              startIndex: rowIndex0,
              endIndex: rowIndex0 + 1,
            },
            inheritFromBefore: false,
          },
        },
      ],
    },
  });
}

export type InterviewRowInput = {
  candidateName: string;
  interviewStage: string;
  location: string;
  interviewPlatform: string;
  poc: string;
  meetingDate: string;
  meetingTime: string;
  meetingDuration: string;
  client: string;
  vendor: string;
  panel: string;
  specialNote: string;
  submitterEmail: string;
  resumeUrl: string;
  jdUrl: string;
  folderUrl: string;
};

export async function appendInterviewRequestRow(
  input: InterviewRowInput
): Promise<{ sheetName: string; row: number }> {
  const sheetName = resolvePocSheet(input.poc);
  const spreadsheetId = GOOGLE_CONFIG.connectorSpreadsheetId;
  const row = GOOGLE_CONFIG.connectorDataStartRow;
  await insertRowAt(spreadsheetId, sheetName, row - 1);

  const values: (string | number)[] = [
    formatTimestamp(),
    input.candidateName,
    input.interviewStage,
    input.location,
    input.interviewPlatform,
    input.poc,
    input.meetingDate,
    formatMeetingTime(input.meetingTime),
    input.meetingDuration,
    input.client,
    input.vendor,
    input.panel,
    GOOGLE_CONFIG.statusDefault,
    input.specialNote || "",
    "",
    "",
    "",
    input.submitterEmail,
  ];

  const sheets = await getSheets();
  await sheets.spreadsheets.values.update({
    spreadsheetId,
    range: `'${sheetName}'!A${row}:R${row}`,
    valueInputOption: "USER_ENTERED",
    requestBody: { values: [values] },
  });

  await writeConnectorHiddenUrls(
    spreadsheetId,
    sheetName,
    row,
    input.resumeUrl,
    input.jdUrl,
    input.folderUrl
  );
  await restoreConnectorHyperlinks(
    spreadsheetId,
    sheetName,
    GOOGLE_CONFIG.connectorDataStartRow
  );

  return { sheetName, row };
}

export type PhoneCallRowInput = {
  poc: string;
  client: string;
  tech: string;
  location: string;
  visa: string;
  candidateName: string;
  meetingDate: string;
  meetingTime: string;
  meetingDuration: string;
  panel: string;
  specialNote: string;
  submitterEmail: string;
  resumeUrl: string;
  jdUrl: string;
  folderUrl: string;
};

/** Phone calls tab — new rows insert at row 2 (below header). */
export async function appendPhoneCallRow(
  input: PhoneCallRowInput
): Promise<{ row: number }> {
  const spreadsheetId = GOOGLE_CONFIG.dataInterviewSpreadsheetId;
  const sheetName = GOOGLE_CONFIG.phoneCallsSheet;
  const row = 2;
  await insertRowAt(spreadsheetId, sheetName, row - 1);

  const values: (string | number)[] = [
    "",
    input.poc,
    input.client,
    input.tech || "Data",
    input.location,
    input.visa,
    input.candidateName,
    input.meetingDate,
    formatMeetingTime(input.meetingTime),
    input.meetingDuration,
    input.panel,
    "",
    GOOGLE_CONFIG.statusDefault,
    "",
    "",
    "",
    input.specialNote || "",
    "",
    input.submitterEmail,
  ];

  const sheets = await getSheets();
  await sheets.spreadsheets.values.update({
    spreadsheetId,
    range: `'${sheetName}'!A${row}:S${row}`,
    valueInputOption: "USER_ENTERED",
    requestBody: { values: [values] },
  });

  await writePhoneCallsHiddenUrls(
    spreadsheetId,
    sheetName,
    row,
    input.jdUrl,
    input.resumeUrl,
    input.folderUrl
  );
  await restorePhoneCallsHyperlinks(spreadsheetId, sheetName);

  return { row };
}

export function isPhoneCallStage(stage: string) {
  return /^phone\s*call$/i.test(String(stage || "").trim());
}
