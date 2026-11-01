import { NetworkShareForm } from "../features/shares/NetworkShareForm";
import type { ShareFormStart, ShareMode } from "../features/shares/NetworkShareForm";
import type { NetworkShare, ShareSupport } from "../features/shares/sharesBackend";
import { Group, Row, Stage } from "./GalleryPageParts";

const WINDOWS: ShareSupport = { supported: true, system: "windows", mount_base: null };
const LINUX: ShareSupport = { supported: true, system: "linux", mount_base: "/run/user/1000/gvfs" };
const NO_MOUNTING: ShareSupport = { ...LINUX, supported: false };
const CONNECT: ShareMode = { kind: "connect", library: "movie" };
const OFFLINE: NetworkShare = {
  id: 1,
  address: "\\\\nas\\movies",
  user: "alex",
  remember: true,
  reason: "wrong_password",
};
const FILLED: ShareFormStart = {
  address: "\\\\nas\\movies\\Films",
  user: "alex",
  password: "examplepassword",
};

interface Specimen {
  label: string;
  mode?: ShareMode;
  support?: ShareSupport;
  start?: ShareFormStart;
}

const SPECIMENS: readonly Specimen[] = [
  { label: "Empty" },
  { label: "Filled", start: FILLED },
  { label: "Connecting", start: { ...FILLED, busy: true } },
  {
    label: "Wrong password",
    start: { ...FILLED, problem: { field: "password", title: "Wrong user name or password." } },
  },
  {
    label: "Server not reachable",
    start: {
      ...FILLED,
      problem: {
        field: "address",
        title: "nas could not be reached. Check the name or address and that it is switched on.",
      },
    },
  },
  {
    label: "Another Windows login to the server",
    start: {
      ...FILLED,
      problem: {
        field: "banner",
        title: "Windows is already connected to nas as another user.",
        body: "Windows allows one login per server. Disconnect the other connection in File Explorer, then connect again.",
      },
    },
  },
  {
    label: "No keyring (Linux)",
    support: LINUX,
    start: {
      ...FILLED,
      problem: {
        field: "banner",
        title: "No system keyring is running, so the password can't be remembered.",
        body: "Start your desktop's keyring and try again, or connect now and enter the password again after each restart.",
        withoutRemembering: true,
      },
    },
  },
  { label: "Connect again", mode: { kind: "again", share: OFFLINE } },
  { label: "Linux without share mounting", support: NO_MOUNTING },
];

/** The Network share page in each state: the real form, with its state set from outside. */
export function ShareSections() {
  return (
    <Group
      title="Network share page"
      note="The form of Settings › Library folders › Network share…: empty, filled, connecting, each refusal, connecting an offline share again, and a Linux desktop without share mounting."
    >
      {SPECIMENS.map(({ label, mode = CONNECT, support = WINDOWS, start }) => (
        <Row key={label} label={label} className="block">
          <Stage tone="window" className="p-4">
            <NetworkShareForm
              mode={mode}
              support={support}
              start={start}
              standalone={false}
              onDone={() => undefined}
              onCancel={() => undefined}
              onAddFolder={() => undefined}
            />
          </Stage>
        </Row>
      ))}
    </Group>
  );
}
