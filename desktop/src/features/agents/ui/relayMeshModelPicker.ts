import {
  AUTO_MODEL_DROPDOWN_VALUE,
  CUSTOM_MODEL_DROPDOWN_VALUE,
  getModelSelectValue,
  hasPersonaModelOption,
  type PersonaDropdownOption,
  type PersonaModelOption,
} from "./personaDialogPickers";

export function relayMeshModelPickerState({
  discoveredOptions,
  fallbackOptions,
  knownOptions,
  isCustomEditing,
  model,
  modelFieldVisible = true,
  provider,
}: {
  discoveredOptions: readonly PersonaModelOption[] | null;
  fallbackOptions: readonly PersonaModelOption[];
  knownOptions?: readonly PersonaModelOption[];
  isCustomEditing: boolean;
  model: string;
  modelFieldVisible?: boolean;
  provider: string;
}) {
  const isRelayMesh = provider.trim() === "relay-mesh";
  const trimmedModel = model.trim();
  const options =
    discoveredOptions ??
    (isRelayMesh ? [{ id: "", label: "Default (auto)" }] : fallbackOptions);
  const isCustom =
    !(isRelayMesh && trimmedModel === "auto") &&
    !hasPersonaModelOption(knownOptions ?? options, model);
  const selectValue =
    isRelayMesh && trimmedModel === "auto"
      ? AUTO_MODEL_DROPDOWN_VALUE
      : getModelSelectValue({
          isCustomModelEditing: isCustomEditing,
          isModelCustom: isCustom,
          model,
        });
  return {
    isCustom,
    isRelayMesh,
    options,
    selectValue,
    showCustomInput:
      !isRelayMesh && modelFieldVisible && (isCustomEditing || isCustom),
  };
}

export function modelDropdownOptions({
  options,
  loading,
  loadingValue,
  allowCustom,
}: {
  options: readonly PersonaModelOption[];
  loading: boolean;
  loadingValue: string;
  allowCustom: boolean;
}): PersonaDropdownOption[] {
  return [
    ...options.map((option) => ({
      label: option.label,
      value: option.id || AUTO_MODEL_DROPDOWN_VALUE,
    })),
    ...(loading
      ? [{ disabled: true, label: "Loading models...", value: loadingValue }]
      : []),
    ...(allowCustom
      ? [{ label: "Custom model...", value: CUSTOM_MODEL_DROPDOWN_VALUE }]
      : []),
  ];
}
