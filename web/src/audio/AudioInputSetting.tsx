// Settings: the audio input remembered for this browser (also chosen in the
// recorder).
import { useCallback, useEffect, useState } from 'react'
import { Button } from '../ui/Button'
import { SelectField } from '../ui/Field'
import { getSavedDevice, listInputs, saveDevice, type InputDevice } from './devices'

export function AudioInputSetting() {
  const [devices, setDevices] = useState<InputDevice[]>([{ id: '', label: 'System default' }])
  const [selected, setSelected] = useState(getSavedDevice)
  const refresh = useCallback(() => {
    void listInputs().then(setDevices)
  }, [])
  useEffect(refresh, [refresh])
  return (
    <div className="recorder__device">
      <SelectField
        label="Input device"
        value={devices.some((device) => device.id === selected) ? selected : ''}
        onChange={(event) => {
          setSelected(event.target.value)
          saveDevice(event.target.value)
        }}
      >
        {devices.map((device) => (
          <option key={device.id} value={device.id}>
            {device.label}
          </option>
        ))}
      </SelectField>
      <Button size="sm" onClick={refresh}>
        Refresh
      </Button>
    </div>
  )
}
