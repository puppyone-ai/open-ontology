import type { Meta, StoryObj } from "@storybook/react-vite";
import { EntityPage } from "./pages";

const meta = {
  title: "Admin/EntityPage",
  component: EntityPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof EntityPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
