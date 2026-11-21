import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";
// Calendar navigation waits for animation frames before React commits the view.
// Shared CI runners can need longer than Testing Library's one-second default.
configure({ asyncUtilTimeout: 5000 });
afterEach(cleanup);
