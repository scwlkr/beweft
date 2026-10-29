import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { readAuthEpoch, setApiWorkspaceEpoch } from "../../src/api";
import { usePendingSchedule } from "../../src/usePendingSchedule";

function Pending({ brandId }: { brandId: string }) {
  const { ready, error } = usePendingSchedule(brandId);
  const history = window as typeof window & {
    pendingFixtureStates: { brandId: string; ready: boolean }[];
  };
  history.pendingFixtureStates ??= [];
  history.pendingFixtureStates.push({ brandId, ready });
  return (
    <p>
      {error
        ? `Error: ${error}`
        : ready
          ? `Ready: ${brandId}`
          : `Checking: ${brandId}`}
    </p>
  );
}

function Fixture() {
  const [visible, setVisible] = useState(true);
  const [brandId, setBrandId] = useState("first");
  return (
    <>
      <button onClick={() => setVisible((value) => !value)}>
        {visible ? "Unmount" : "Mount"}
      </button>
      <button
        onClick={() =>
          setBrandId((id) => (id === "first" ? "second" : "first"))
        }
      >
        Change brand
      </button>
      {visible && <Pending brandId={brandId} />}
    </>
  );
}

// The real app establishes this after its workspace check. This isolated fixture
// renders the otherwise unwired helper to test its own lifecycle.
setApiWorkspaceEpoch(readAuthEpoch());
createRoot(document.getElementById("root")!).render(<Fixture />);
