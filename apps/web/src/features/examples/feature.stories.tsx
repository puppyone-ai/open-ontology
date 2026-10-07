import type { Meta, StoryObj } from "@storybook/react-vite";
import { ExamplesPage } from "./pages";

const meta = {
  title: "Admin/ExamplesPage",
  component: ExamplesPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ExamplesPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
