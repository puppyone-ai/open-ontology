import { useErrorText } from "../../hooks/useText";
import { resolveErrorMessageId, type SuperDriveError } from "../../model/superDriveErrors";
import { hasSuperDriveMessage } from "../../model/superDriveMessageKeys";

/** An error of the engine or the bridge: the sentence in the language of the app, and the code on a details line (the generic sentence carries it already). */
export function ErrorText({ error }: { error: SuperDriveError }) {
  const errorText = useErrorText();
  const generic = resolveErrorMessageId(error.code, hasSuperDriveMessage) === "super-drive.errors.unknown";
  return (
    <>
      {errorText(error)}
      {!generic && <span className="so-error-code so-mono" data-testid="error-code">{error.code}</span>}
    </>
  );
}
