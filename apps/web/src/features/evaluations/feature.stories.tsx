import type { Meta, StoryObj } from "@storybook/react-vite";
import { EvaluationDetailPage } from "./pages";

const meta = {
  title: "Admin/EvaluationDetailPage",
  component: EvaluationDetailPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof EvaluationDetailPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
