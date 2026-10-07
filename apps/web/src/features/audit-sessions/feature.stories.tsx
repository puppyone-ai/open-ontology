import type { Meta, StoryObj } from "@storybook/react-vite";
import { AuditPage } from "./pages";

const meta = {
  title: "Admin/AuditPage",
  component: AuditPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof AuditPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
