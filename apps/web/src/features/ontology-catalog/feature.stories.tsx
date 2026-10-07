import type { Meta, StoryObj } from "@storybook/react-vite";
import { OntologyPage } from "./pages";

const meta = {
  title: "Admin/OntologyPage",
  component: OntologyPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof OntologyPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
