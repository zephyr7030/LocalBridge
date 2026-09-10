import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { ModalSurface } from "../components/ModalSurface";

it("renders one shared modal contract with explicit priority", () => {
  const rendered = renderToStaticMarkup(
    <ModalSurface
      className="confirmation-dialog"
      backdropClassName="confirmation-backdrop"
      priority={30}
      ariaLabel="Confirmation"
    >
      <button>Reject</button>
      <button>Approve</button>
    </ModalSurface>,
  );

  expect(rendered).toContain('class="dialog-backdrop confirmation-backdrop"');
  expect(rendered).toContain('data-modal-priority="30"');
  expect(rendered).toContain('class="dialog confirmation-dialog"');
  expect(rendered).toContain('role="dialog"');
  expect(rendered).toContain('aria-modal="true"');
  expect(rendered).toContain('aria-label="Confirmation"');
  expect(rendered).toContain('tabindex="-1"');
});

it("keeps sheet presentation on the same modal behavior owner", () => {
  const rendered = renderToStaticMarkup(
    <ModalSurface variant="sheet" ariaLabel="Settings">
      <button>Done</button>
    </ModalSurface>,
  );

  expect(rendered).toContain('class="sheet-backdrop"');
  expect(rendered).toContain('data-modal-priority="10"');
  expect(rendered).toContain('class="sheet"');
});
