import type { Meta, StoryObj } from "@storybook/react-vite";
import { CalendarPage } from "./pages";

const meta = {
  title: "Admin/CalendarPage",
  component: CalendarPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof CalendarPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
